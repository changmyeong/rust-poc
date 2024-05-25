use near_contract_standards::fungible_token::{receiver::FungibleTokenReceiver, Balance};
use near_sdk::{collections::LookupMap, env, json_types::U128, log, near, require, serde::{Deserialize, Serialize}, serde_json, AccountId, PanicOnDefault, PromiseOrValue};

use crate::{core::PoolCore, DelegaterInfo, VerticalAPI};

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct PoolContract {
    // vertical api's id => pool info
    vertical_apis: LookupMap<String, VerticalAPI>,
    token_id: AccountId,
    owner_id: AccountId,
}


#[near]
impl PoolContract {
    #[init]
    pub fn new(token_id: AccountId, owner_id: AccountId) -> Self {
        Self {
            vertical_apis: LookupMap::new(b"v".to_vec()),
            token_id,
            owner_id,
        }
    }
}

#[near]
impl PoolCore for PoolContract {
    fn create_sample_vapi(&mut self) {
        let vapi_id = "test-vapi".to_string();
        let sender_id = env::predecessor_account_id();
        let vertical_api = VerticalAPI::new(sender_id);
        self.vertical_apis.insert(&vapi_id, &vertical_api);
    }

    fn claim_reward(&mut self, sender_id: &AccountId, vapi_id: String) -> Balance {
        let reward_amount = self.pending_reward(sender_id, vapi_id.clone());
        if reward_amount == 0 {
            return 0;
        }

        let mut vertical_api = self.vertical_apis.get(&vapi_id).unwrap();
        let mut delegater_info = vertical_api.delegation.delegaters.get(sender_id).unwrap();
        
        delegater_info.reward_debt = delegater_info.deposit_amount * vertical_api.delegation.acc_reward_per_share / 1_000_000_000_000;
        vertical_api.delegation.delegaters.insert(sender_id, &delegater_info);
        
        log!("[claim_rewrd] reward_amount: {}", reward_amount);

        self.vertical_apis.insert(&vapi_id, &vertical_api);
        return reward_amount;
    }

    fn pending_reward(&self, sender_id: &AccountId, vapi_id: String) -> Balance {
        log!("[pending_reward]");
        let vertical_api = self.vertical_apis.get(&vapi_id).expect("Vertical API not found");
        log!("[pending_reward] found vertical_api");
        let delegater_info = vertical_api.delegation.delegaters.get(sender_id).or(Some(DelegaterInfo {
            deposit_amount: 0,
            reward_debt: 0,
        })).unwrap();

        log!("[pending_reward] deposit_amount: {}, reward_debt: {}", delegater_info.deposit_amount, delegater_info.reward_debt);
        log!("[pending_reward] acc_reward_per_share: {}", vertical_api.delegation.acc_reward_per_share);

        return (delegater_info.deposit_amount * vertical_api.delegation.acc_reward_per_share / 1_000_000_000_000) - delegater_info.reward_debt;
    }
}

impl PoolContract {
    fn internal_deposit(&mut self, sender_id: &AccountId, vapi_id: String, amount: Balance) {
        log!("[internal_deposit] deposit to vapi: {}", amount);
        let reward_amount = self.claim_reward(sender_id, vapi_id.clone());

        let vertical_api = self.vertical_apis.get(&vapi_id).expect("Vertical API not found");
        log!("[internal_deposit] found vertical_api");
        
        let mut delegation = vertical_api.delegation;
        delegation.total_deposit_amount += amount;

        if reward_amount > 0 {
            delegation.acc_reward_per_share += reward_amount * 1_000_000_000_000 / delegation.total_deposit_amount;
        }

        let mut delegater_info = delegation.delegaters.get(&sender_id).unwrap_or(DelegaterInfo {
            deposit_amount: 0,
            reward_debt: 0,
        });
        delegater_info.deposit_amount += amount;
        delegater_info.reward_debt = delegater_info.deposit_amount * delegation.acc_reward_per_share / 1_000_000_000_000;
        delegation.delegaters.insert(&sender_id, &delegater_info);

        log!("[internal_deposit] success: {}", delegation.total_deposit_amount);
        self.vertical_apis.insert(&vapi_id, &VerticalAPI {
            developer: vertical_api.developer,
            delegation,
        });
    }

    fn internal_settlement(&mut self, vapi_id: String, amount: Balance) {
        log!("[internal_settlement]");
        let vertical_api = self.vertical_apis.get(&vapi_id).expect("Vertical API not found");
        let mut delegation = vertical_api.delegation;
        let total_deposit_amount = delegation.total_deposit_amount;
        log!("[internal_settlement] total_deposit_amount: {}", total_deposit_amount);
        if total_deposit_amount == 0 {
            return;
        }

        delegation.acc_reward_per_share += amount * 1_000_000_000_000 / total_deposit_amount;
        delegation.last_reward_timestamp = env::block_timestamp();
        self.vertical_apis.insert(&vapi_id, &VerticalAPI {
            developer: vertical_api.developer,
            delegation,
        });
        log!("[internal_settlement] success");
    }
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
#[serde(untagged)]
enum TokenReceiverMessage {
    Deposit {
        vapi_id: String,
    },
}

#[near]
impl FungibleTokenReceiver for PoolContract {
  fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) -> PromiseOrValue<U128> {
      let token_id: AccountId = env::predecessor_account_id();
      require!(token_id == self.token_id, "Invalid token");

      if msg.is_empty() {
          PromiseOrValue::Value(U128(0));
      }

      log!("[ft_on_transfer] msg: {}", msg);
      let message = serde_json::from_str::<TokenReceiverMessage>(&msg).expect("Invalid message format");
      log!("[ft_on_transfer] selected message");

      match message {
          TokenReceiverMessage::Deposit { vapi_id } => {
              if sender_id.ne(&self.owner_id)  {
                  self.internal_deposit(&sender_id, vapi_id, amount.into());
              } else {
                  self.internal_settlement(vapi_id, amount.into());
              }
          }
      }

      return PromiseOrValue::Value(U128(0));
  }
}