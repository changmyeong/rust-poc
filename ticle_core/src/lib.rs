use near_contract_standards::fungible_token::core::ext_ft_core;
use near_contract_standards::fungible_token::resolver::ext_ft_resolver;
use near_contract_standards::fungible_token::Balance;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, ext_contract, log, near, require, serde_json, AccountId, Gas, NearToken, PanicOnDefault, Promise, PromiseOrValue, PromiseResult};
use near_sdk::collections::{LookupMap, UnorderedMap};
use near_sdk::json_types::U128;
use ed25519_dalek::{PublicKey, Signature, Verifier};

pub mod ft_receiver;

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct TicleCore {
    vapis: LookupMap<String, VAPI>,
    token_id: AccountId,
    owner_id: AccountId,
    signer_public_key: Vec<u8>,
}

#[near(serializers = [borsh])]
pub struct VAPI { 
    coder_info: CoderInfo,
    reviewer_infos: UnorderedMap<AccountId, ReviewerInfo>,
	delegation_info: DelegationInfo,
}

#[near(serializers = [borsh])]
pub struct CoderInfo {
    account_id: AccountId,
    unclaimed_reward_amount: Balance,
}

#[near(serializers = [borsh])]
pub struct DelegationInfo {
	delegator_infos: LookupMap<AccountId, Delegator>,
	total_deposit_amount: Balance,
	acc_reward_per_share: Balance,
}

#[near(serializers = [borsh, json])]
pub struct ReviewerInfo {
    version: String,
	royalty_amount: Balance,
    timestamp: u64,
}

#[near(serializers = [borsh])]
pub struct Delegator {
    deposit_amount: Balance,
    reward_debt: Balance,
}

#[ext_contract(ext_ft_burn)]
pub trait FungibleTokenBurn {
    fn burn(&mut self, amount: U128);
}

#[near]
impl TicleCore {
    #[init]
    pub fn new(token_id: AccountId, owner_id: AccountId) -> Self {
        let mut signer_public_key = env::signer_account_pk().into_bytes();

        // HACK: prefix로 맨 앞에 0이 붙어서 33바이트가 되는 경우가 있음
        if signer_public_key.len() > 32 {
            signer_public_key.remove(0);
        }
        Self {
            vapis: LookupMap::new(b"v".to_vec()),
            token_id,
            owner_id,
            signer_public_key,
        }
    }
}

#[near]
impl TicleCore {
    pub fn create_vapi(&mut self, vapi_id: String) {
        let coder_id = env::predecessor_account_id();
        let vapi = VAPI {
            coder_info: CoderInfo {
                account_id: coder_id,
                unclaimed_reward_amount: 0,
            },
            reviewer_infos: UnorderedMap::new(b"r".to_vec()),
            delegation_info: DelegationInfo {
                delegator_infos: LookupMap::new(b"d".to_vec()),
                total_deposit_amount: 0,
                acc_reward_per_share: 0,
            },
        };
        self.vapis.insert(&vapi_id, &vapi);
    }

    pub fn cancel_review(&mut self, vapi_id: String, reviewer_ids: Vec<AccountId>) -> Promise {
        require!(reviewer_ids.len() > 0, "At least one reviewer must be provided");
        require!(reviewer_ids.len() <= 3, "Maximum 3 reviewers can be cancelled");
        let mut vapi = self.vapis.get(&vapi_id).expect("VAPI not found");

        let coder_id = env::predecessor_account_id();
        require!(vapi.coder_info.account_id == coder_id, "Only coder can cancel review");

        let mut combined_promise: Option<Promise> = None;
        let contract_id = env::current_account_id();
        for reviewer_id in reviewer_ids {
            let reviewer_info = vapi.reviewer_infos.get(&reviewer_id).expect("Reviewer not found");
            let royalty_amount = U128(reviewer_info.royalty_amount);
            
            let promise = ext_ft_core::ext(self.token_id.clone())
                .with_attached_deposit(NearToken::from_yoctonear(1))
                .with_static_gas(Gas::from_tgas(20))
                .ft_transfer(reviewer_id.clone(), royalty_amount, None)
                .then(
                    ext_ft_resolver::ext(contract_id.clone())
                        .with_static_gas(Gas::from_tgas(10))
                        .ft_resolve_transfer(contract_id.clone(), reviewer_id.clone(), royalty_amount),
                );
            
            if let Some(existing_promise) = combined_promise {
                combined_promise = Some(existing_promise.and(promise));
            } else {
                combined_promise = Some(promise);
            }
            
            vapi.reviewer_infos.remove(&reviewer_id);
        }

        self.vapis.insert(&vapi_id, &vapi);
        return combined_promise
            .unwrap()
            .then(
                Self::ext(contract_id.clone())
                    .with_static_gas(Gas::from_tgas(20))
                    .callback_cancel_review()
            );
    }

    #[private]
    pub fn callback_cancel_review(&self) {
        let promise_count = env::promise_results_count();
        assert!(promise_count > 0, "Expected at least one promise result.");

        for i in 0..promise_count {
            match env::promise_result(i) {
                PromiseResult::Successful(_) => {
                    // do nothing
                }
                _ => env::panic_str("ft_transfer failed during cancel review"),
            }
        }
    }

    // TODO: 삭제해야 하는지 확인해보기
    pub fn transfer_ownership(&mut self, vapi_id: String, new_coder_id: AccountId) {
        let mut vapi = self.vapis.get(&vapi_id).expect("Vertical API not found");

        let account_id = env::predecessor_account_id();
        require!(vapi.coder_info.account_id == account_id, "Only coder can transfer ownership");

        vapi.coder_info.account_id = new_coder_id;
        self.vapis.insert(&vapi_id, &vapi);
    }

    pub fn claim_reward(&mut self, vapi_id: String) -> Promise {
        let sender_id = env::predecessor_account_id();
        return self.internal_claim_reward(&sender_id, vapi_id.clone());
    }

    pub fn withdraw(&self, vapi_id: String, amount: U128) -> Promise {
        let sender_id = env::predecessor_account_id();
        return self.internal_claim_reward(&sender_id, vapi_id.clone()).then(
            Self::ext(env::current_account_id())
                .with_static_gas(Gas::from_tgas(20))
                .callback_withdraw(&sender_id, vapi_id.clone(), amount)
        );
    }

    pub fn claim_review_reward(&self, vapi_id: String) -> Promise {
        let sender_id = env::predecessor_account_id();
        let vapi = self.vapis.get(&vapi_id).expect("Vertical API not found");
        let reviewer_info = vapi.reviewer_infos.get(&sender_id).expect("Reviewer not found");
        
        let timestamp = reviewer_info.timestamp;
        let current_timestamp = env::block_timestamp();

        const TWO_WEEKS: u64 = 60 * 60 * 24 * 14 * 1000000;
        log!("[claim_review_reward] current_timestamp: {}, timestamp: {}", current_timestamp, timestamp);
        require!(current_timestamp >= timestamp + TWO_WEEKS, "Reviewer can claim reward after 2 weeks");

        return ext_ft_core::ext(self.token_id.clone())
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(Gas::from_tgas(20))
            .ft_transfer(sender_id.clone(), U128(reviewer_info.royalty_amount), None)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(Gas::from_tgas(20))
                    .callback_claim_review_reward(sender_id.clone(), vapi_id.clone())
            );
    }

    #[private]
    pub fn callback_pending_reward(&mut self, sender_id: &AccountId, vapi_id: String) {
        let mut vapi = self.vapis.get(&vapi_id).unwrap();
        let mut delegator_info = vapi.delegation_info.delegator_infos.get(sender_id).unwrap();
        
        delegator_info.reward_debt = delegator_info.deposit_amount * vapi.delegation_info.acc_reward_per_share / 1_000_000_000_000;
        vapi.delegation_info.delegator_infos.insert(sender_id, &delegator_info);

        self.vapis.insert(&vapi_id, &vapi);
    }

    #[private]
    pub fn callback_withdraw(&mut self, sender_id: &AccountId, vapi_id: String, amount: U128) -> Promise {
        let mut vapi = self.vapis.get(&vapi_id).expect("Vertical API not found");
        let mut delegator_info = vapi.delegation_info.delegator_infos.get(sender_id).expect("Delegator not found");
        delegator_info.deposit_amount -= amount.0;

        if delegator_info.deposit_amount == 0 {
            vapi.delegation_info.delegator_infos.remove(sender_id);
        }
        vapi.delegation_info.total_deposit_amount -= amount.0;
        self.vapis.insert(&vapi_id, &vapi);

        return ext_ft_core::ext(self.token_id.clone())
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(Gas::from_tgas(20))
            .ft_transfer(sender_id.clone(), amount, None);
    }

    pub fn pending_reward(&self, sender_id: &AccountId, vapi_id: String) -> Balance {
        log!("[pending_reward] {}", sender_id);
        let vapi = self.vapis.get(&vapi_id).expect("Vertical API not found");
        log!("[pending_reward] found vertical_api");
        let delegator_info = vapi.delegation_info.delegator_infos.get(&sender_id).unwrap_or(Delegator {
            deposit_amount: 0,
            reward_debt: 0,
        });

        log!("[pending_reward] deposit_amount: {}, reward_debt: {}", delegator_info.deposit_amount, delegator_info.reward_debt);
        log!("[pending_reward] acc_reward_per_share: {}", vapi.delegation_info.acc_reward_per_share);

        return (delegator_info.deposit_amount * vapi.delegation_info.acc_reward_per_share / 1_000_000_000_000) - delegator_info.reward_debt;
    }
}

#[near]
impl TicleCore {
    fn internal_deposit(&mut self, sender_id: &AccountId, vapi_id: String, amount: Balance) -> Promise {
        log!("[internal_deposit] deposit to vapi: {}", vapi_id);
        
        let contract_id = env::current_account_id();
        return self.internal_claim_reward(sender_id, vapi_id.clone()).then(
            Self::ext(contract_id.clone())
                .with_static_gas(Gas::from_tgas(20))
                .callback_internal_deposit(sender_id.clone(), vapi_id, amount)
        )
    }

    #[private]
    pub fn callback_internal_deposit(&mut self, sender_id: AccountId, vapi_id: String, amount: Balance) {
        let mut vapi = self.vapis.get(&vapi_id).expect("VAPI not found");
        log!("[callback_internal_deposit] found vertical_api");
        
        let mut delegation_info = vapi.delegation_info;
        delegation_info.total_deposit_amount += amount;
        
        let reward_amount = self.pending_reward(&sender_id, vapi_id.clone());
        if reward_amount > 0 {
            delegation_info.acc_reward_per_share += reward_amount * 1_000_000_000_000 / delegation_info.total_deposit_amount;
        }
        
        let mut delegator_info = delegation_info.delegator_infos.get(&sender_id).unwrap_or(Delegator {
            deposit_amount: 0,
            reward_debt: 0,
        });
        delegator_info.deposit_amount += amount;
        delegator_info.reward_debt = delegator_info.deposit_amount * delegation_info.acc_reward_per_share / 1_000_000_000_000;
        
        delegation_info.delegator_infos.insert(&sender_id, &delegator_info);
        vapi.delegation_info = delegation_info;
        self.vapis.insert(&vapi_id, &vapi);

        log!("[callback_internal_deposit] success: {}", vapi.delegation_info.total_deposit_amount);
    }

    #[private]
    pub fn callback_claim_review_reward(&mut self, sender_id: AccountId, vapi_id: String) {
        let mut vapi = self.vapis.get(&vapi_id).expect("VAPI not found");
        vapi.reviewer_infos.remove(&sender_id);
        self.vapis.insert(&vapi_id, &vapi);
    }

    fn internal_settlement (&mut self, sender_id: &AccountId, vapi_ids: Vec<String>, amounts: Vec<U128>) -> Promise {
        log!("[internal_settlement]");
        require!(*sender_id == self.owner_id, "Only owner can settle");
        require!(vapi_ids.len() == amounts.len(), "vapi_ids and amounts must have the same length");

        let mut total_burn_amount: u128 = 0;
        for (vapi_id, amount) in vapi_ids.iter().zip(amounts.iter()) {
            let amount: Balance = amount.0;
            let delegator_fee_amount = amount * 39 / 100;
            let burn_amount = amount * 1 / 100;

            let mut vapi = self.vapis.get(&vapi_id).expect("VAPI not found");
            vapi.coder_info.unclaimed_reward_amount += amount - delegator_fee_amount - burn_amount;
            vapi.delegation_info.acc_reward_per_share += delegator_fee_amount * 1_000_000_000_000 / vapi.delegation_info.total_deposit_amount;
            self.vapis.insert(&vapi_id, &vapi);

            total_burn_amount += burn_amount;
        }

        return ext_ft_burn::ext(self.token_id.clone())
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(Gas::from_tgas(20))
            .burn(U128(total_burn_amount));
    }

    fn internal_request_review(
        &mut self,
        vapi_id: String,
        version: String,
        reviewer_ids: Vec<AccountId>,
        royalty_amounts: Vec<U128>,
        transfer_sender_id: AccountId,
        transfer_amount: U128,
        signature: String,
    ) {
        log!("[internal_request_review]");
        require!(reviewer_ids.len() == royalty_amounts.len(), "reviewer_ids and royalty_amounts must have the same length");
        let mut vapi = self.vapis.get(&vapi_id).expect("VAPI not found");
        require!(vapi.coder_info.account_id == transfer_sender_id, "Only coder can request review");

        let message = format!("{},{},{:?},{:?}", vapi_id, version, reviewer_ids, royalty_amounts);
        let message_bytes = message.as_bytes();
        require!(self.verify_signature(message_bytes, signature), "Invalid signature");

        let timestamp = env::block_timestamp();
        let mut total_royalty_amount: Balance = 0;
        for (reviewer_id, royalty_amount) in reviewer_ids.iter().zip(royalty_amounts.iter()) {
            let royalty_amount: Balance = royalty_amount.0;
            total_royalty_amount += royalty_amount;
            vapi.reviewer_infos.insert(&reviewer_id, &ReviewerInfo {
                version: version.clone(),
                royalty_amount,
                timestamp,
            });
        }

        log!("[internal_request_review] total_royalty_amount: {}", total_royalty_amount);
        log!("[internal_request_review] transfer_amount: {}", transfer_amount.0);

        require!(total_royalty_amount == transfer_amount.0, "Invalid amount");

        self.vapis.insert(&vapi_id, &vapi);
    }

    fn internal_claim_reward(&self, sender_id: &AccountId, vapi_id: String) -> Promise {
        let reward_amount = self.pending_reward(&sender_id, vapi_id.clone());
        if reward_amount == 0 {
            log!("[claim_rewrd] no claim");
            return Promise::new(env::current_account_id());
        }

        log!("[claim_rewrd] reward_amount: {}", reward_amount);
        let contract_id = env::current_account_id();
        return ext_ft_core::ext(self.token_id.clone())
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(Gas::from_tgas(20))
            .ft_transfer(sender_id.clone(), U128(reward_amount), None).then(
                Self::ext(contract_id.clone())
                    .with_static_gas(Gas::from_tgas(20))
                    .callback_pending_reward(&sender_id, vapi_id.clone())
            )
            .into();
    }

    fn verify_signature(&self, message: &[u8], signature: String) -> bool {
        let signature_base58 = signature.trim_start_matches("ed25519:");

        // Decode the base58 signature
        let signature_bytes = match bs58::decode(&signature_base58).into_vec() {
            Ok(bytes) => bytes,
            Err(_) => {
                log!("[verify_signature] Invalid base58 in signature");
                return false;
            }
        };

        // Convert the stored public key bytes to a PublicKey object
        let public_key = match PublicKey::from_bytes(&self.signer_public_key) {
            Ok(pk) => pk,
            Err(_) => {
                log!("[verify_signature] Invalid public key");
                return false;
            }
        };

        // Convert the signature bytes to a Signature object
        let signature = match Signature::from_bytes(&signature_bytes) {
            Ok(sig) => sig,
            Err(_) => {
                log!("[verify_signature] Invalid signature");
                return false;
            }
        };

        // Verify the signature
        match public_key.verify(message, &signature) {
            Ok(_) => true,
            Err(e) => {
                log!("[verify_signature] verification error: {:?}", e);
                false
            }
        }
    }
}