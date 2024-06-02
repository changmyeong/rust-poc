use near_contract_standards::fungible_token::core::ext_ft_core;
use near_contract_standards::fungible_token::resolver::ext_ft_resolver;
use near_contract_standards::fungible_token::Balance;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, log, near, require, serde_json, AccountId, Gas, NearToken, PanicOnDefault, Promise, PromiseOrValue, PromiseResult};
use near_sdk::collections::{LookupMap, UnorderedMap};
use near_sdk::json_types::U128;

mod ft_receiver;

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Core {
    vapis: LookupMap<String, VAPI>,
    token_id: AccountId,
    owner_id: AccountId,
    reviewers: LookupMap<AccountId, Balance>,
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

// 슬래싱 정책 픽스 후 추가 개발 예정
// 리뷰어 업데이트 가능 여부 확인 필요
// 리뷰 보상은 언제 지급하나? 적어도 슬래싱 여부 판단 기간 정도는 필요함
#[near(serializers = [borsh, json])]
pub struct ReviewerInfo {
    version: String,
	is_approved: bool,
	royalty_amount: Balance,
}

#[near(serializers = [borsh])]
pub struct Delegator {
    deposit_amount: Balance,
    reward_debt: Balance,
}

#[near]
impl Core {
    #[init]
    pub fn new(token_id: AccountId, owner_id: AccountId) -> Self {
        Self {
            vapis: LookupMap::new(b"v".to_vec()),
            token_id,
            owner_id,
            reviewers: LookupMap::new(b"r".to_vec()),
        }
    }
}

#[near]
impl Core {
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
            require!(reviewer_info.is_approved == false, "Reviewer already approved");

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
                    .cancel_review_callback()
            );
    }

    #[private]
    pub fn cancel_review_callback(&self) {
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

    // VAPI 소유권도 이전할 수 있어야 할까?
    // 소유권은 NFT로 발행하는 것이 좋을까?
    pub fn transfer_ownership(&mut self, vapi_id: String, new_coder_id: AccountId) {
        let mut vapi = self.vapis.get(&vapi_id).expect("Vertical API not found");

        let account_id = env::predecessor_account_id();
        require!(vapi.coder_info.account_id == account_id, "Only coder can transfer ownership");

        vapi.coder_info.account_id = new_coder_id;
        self.vapis.insert(&vapi_id, &vapi);
    }

    fn claim_reward(&mut self, sender_id: &AccountId, vapi_id: String) -> Balance {
        let reward_amount = self.pending_reward(sender_id, vapi_id.clone());
        if reward_amount == 0 {
            return reward_amount;
        }

        let mut vapi = self.vapis.get(&vapi_id).unwrap();
        let mut delegater_info = vapi.delegation_info.delegator_infos.get(sender_id).unwrap();
        
        delegater_info.reward_debt = delegater_info.deposit_amount * vapi.delegation_info.acc_reward_per_share / 1_000_000_000_000;
        vapi.delegation_info.delegator_infos.insert(sender_id, &delegater_info);
        
        log!("[claim_rewrd] reward_amount: {}", reward_amount);

        self.vapis.insert(&vapi_id, &vapi);

        return reward_amount;
    }

    fn pending_reward(&self, sender_id: &AccountId, vapi_id: String) -> Balance {
        log!("[pending_reward]");
        let vapi = self.vapis.get(&vapi_id).expect("Vertical API not found");
        log!("[pending_reward] found vertical_api");
        let delegater_info = vapi.delegation_info.delegator_infos.get(sender_id).or(Some(Delegator {
            deposit_amount: 0,
            reward_debt: 0,
        })).unwrap();

        log!("[pending_reward] deposit_amount: {}, reward_debt: {}", delegater_info.deposit_amount, delegater_info.reward_debt);
        log!("[pending_reward] acc_reward_per_share: {}", vapi.delegation_info.acc_reward_per_share);

        return (delegater_info.deposit_amount * vapi.delegation_info.acc_reward_per_share / 1_000_000_000_000) - delegater_info.reward_debt;
    }
}

#[near]
impl Core {
    fn internal_deposit(&mut self, sender_id: &AccountId, vapi_id: String, amount: Balance) {
        log!("[internal_deposit] deposit to vapi: {}", amount);
        let reward_amount = self.claim_reward(sender_id, vapi_id.clone());

        let mut vapi = self.vapis.get(&vapi_id).expect("VAPI not found");
        log!("[internal_deposit] found vertical_api");
        
        vapi.delegation_info.total_deposit_amount += amount;

        if reward_amount > 0 {
            vapi.delegation_info.acc_reward_per_share += reward_amount * 1_000_000_000_000 / vapi.delegation_info.total_deposit_amount;
        }

        let mut delegator_info = vapi.delegation_info.delegator_infos.get(&sender_id).unwrap_or(Delegator {
            deposit_amount: 0,
            reward_debt: 0,
        });
        delegator_info.deposit_amount += amount;
        delegator_info.reward_debt = delegator_info.deposit_amount * vapi.delegation_info.acc_reward_per_share / 1_000_000_000_000;
        vapi.delegation_info.delegator_infos.insert(&sender_id, &delegator_info);

        log!("[internal_deposit] success: {}", vapi.delegation_info.total_deposit_amount);
        self.vapis.insert(&vapi_id, &vapi);
    }

    fn internal_settlement (&mut self, sender_id: &AccountId, vapi_ids: Vec<String>, amounts: Vec<Balance>) {
        require!(*sender_id == self.owner_id, "Only owner can settle");
        require!(vapi_ids.len() == amounts.len(), "vapi_ids and amounts must have the same length");

        for (vapi_id, amount) in vapi_ids.iter().zip(amounts.iter()) {
            let delegator_fee_amount = amount * 39 / 100;
            let burn_amount = amount * 1 / 100;

            let mut vapi = self.vapis.get(&vapi_id).expect("VAPI not found");
            vapi.coder_info.unclaimed_reward_amount += amount - delegator_fee_amount - burn_amount;
            vapi.delegation_info.acc_reward_per_share += delegator_fee_amount * 1_000_000_000_000 / vapi.delegation_info.total_deposit_amount;
            self.vapis.insert(&vapi_id, &vapi);

            // TODO: transfer to coder and burn 1% of the fee
        }
    }

    fn internal_request_review(
        &mut self,
        vapi_id: String,
        reviewer_ids: Vec<AccountId>,
        reviewer_infos: Vec<ReviewerInfo>,
        transfer_sender_id: AccountId,
        transfer_amount: U128
    ) {
        require!(reviewer_ids.len() == reviewer_infos.len(), "reviewer_ids and reviewer_infos must have the same length");
        let mut vapi = self.vapis.get(&vapi_id).expect("VAPI not found");
        require!(vapi.coder_info.account_id == transfer_sender_id, "Only coder can request review");

        let mut total_royalty_amount: Balance = 0;
        for (reviewer_id, reviewer_info) in reviewer_ids.iter().zip(reviewer_infos.iter()) {
            total_royalty_amount += reviewer_info.royalty_amount;
            vapi.reviewer_infos.insert(&reviewer_id, &reviewer_info);
        }

        require!(total_royalty_amount == transfer_amount.into(), "Invalid amount");

        self.vapis.insert(&vapi_id, &vapi);
    }
}