use near_contract_standards::fungible_token::Balance;
use near_sdk::{env, AccountId, near};
use near_sdk::collections::UnorderedMap;

#[near(serializers = [borsh])]
pub struct DelegaterInfo {
    pub deposit_amount: Balance,
    pub reward_debt: Balance,
}

#[near(serializers = [borsh])]
pub struct Delegation {
    pub delegaters: UnorderedMap<AccountId, DelegaterInfo>,
    pub total_deposit_amount: Balance,
    pub acc_reward_per_share: Balance,
    pub last_reward_timestamp: u64,
}

impl Default for DelegaterInfo {
    fn default() -> Self {
        Self {
            deposit_amount: 0,
            reward_debt: 0,
        }
    }
}

#[near(serializers = [borsh])]
pub struct Developer {
    pub account_id: AccountId,
    pub unclaimed_reward: Balance,
}

#[near(serializers = [borsh])]
pub struct VerticalAPI {
    pub developer: Developer,
    pub delegation: Delegation,
}

impl VerticalAPI {
    pub fn new(developer_account_id: AccountId) -> Self {
        Self {
            developer: Developer {
                account_id: developer_account_id,
                unclaimed_reward: 0,
            },
            delegation: Delegation {
                delegaters: UnorderedMap::new(b"de".to_vec()),
                total_deposit_amount: 0,
                acc_reward_per_share: 0,
                last_reward_timestamp: env::block_timestamp(),
            },
        }
    }
}