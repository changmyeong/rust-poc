use near_contract_standards::fungible_token::Balance;
use near_sdk::{ext_contract, AccountId};

#[ext_contract(ext_pool_core)]
pub trait PoolCore {
  fn create_sample_vapi(&mut self) -> ();
  fn claim_reward(&mut self, sender_id: &AccountId, vapi_id: String) -> Balance;
  fn pending_reward(&self, sender_id: &AccountId, vapi_id: String) -> Balance;
}