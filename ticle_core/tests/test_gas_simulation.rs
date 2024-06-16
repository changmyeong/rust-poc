use std::str::FromStr;

use near_sdk::{json_types::U128, NearToken};
use serde_json::json;
use near_crypto::SecretKey;

use crate::common::utils::*;
pub mod common;

#[tokio::test]
async fn test_gas_simulation() -> anyhow::Result<()> {
    // create vapi
    let initial_balance = U128::from(NearToken::from_near(10000).as_yoctonear());
    let worker = near_workspaces::sandbox().await?;
    let (ft_contract, owner, core_contract) = init(&worker, initial_balance).await?;
    
    register_user(&ft_contract, core_contract.id()).await?;

    let users = create_users(&worker, vec!["alice", "bob", "charlie"], vec![10, 10, 10]).await?;
    for user in users.iter() {
        register_user(&ft_contract, user.id()).await?;

        let res = owner.transfer_near(user.id(), NearToken::from_near(1)).await?;
        assert!(res.is_success());

        let res = owner
            .call(ft_contract.id(), "ft_transfer")
            .args_json((user.id(), U128::from(NearToken::from_near(100).as_yoctonear()), "transfer to test account"))
            .max_gas()
            .deposit(ONE_YOCTO)
            .transact()
            .await?;
        assert!(res.is_success());
    }

    let alice = users.get(0).unwrap().clone();
    let bob = users.get(1).unwrap().clone();
    let charlie = users.get(2).unwrap().clone();

    let initial_storage_usage = core_contract.view_account().await?.storage_usage;
    let vapi_id = "test-vapi";
    let res = alice
        .call(core_contract.id(), "create_vapi")
        .args_json(json!({"vapi_id": vapi_id}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());
    let final_storage_usage = core_contract.view_account().await?.storage_usage;

    let storage_used = final_storage_usage - initial_storage_usage;
    println!("[create_vapi] Storage used: {} bytes", storage_used);

    let amount = U128::from(NearToken::from_near(10).as_yoctonear());
    let vapi_version = "1.0";
    let message = format!("{},{},{:?},{:?}", vapi_id, vapi_version, vec![bob.id(), charlie.id()], vec![amount, amount]);
    let message_bytes = message.as_bytes();
    
    let owner_secret_key = SecretKey::from_str(&owner.secret_key().to_string()).unwrap();
    let signature = owner_secret_key.sign(message_bytes).to_string();

    let initial_storage_usage = core_contract.view_account().await?.storage_usage;
    let res = alice
        .call(ft_contract.id(), "ft_transfer_call")
        .args_json((
          core_contract.id(),
          U128::from(NearToken::from_near(20).as_yoctonear()),
          Option::<String>::None,
          serde_json::json!({
            "vapi_id": vapi_id, 
            "version": vapi_version,
            "reviewer_ids": vec![bob.id(), charlie.id()],
            "royalty_amounts": vec![amount, amount], 
            "signature": signature 
          }).to_string()
        ))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;
    assert!(res.is_success());
    let final_storage_usage = core_contract.view_account().await?.storage_usage;
    let storage_used = final_storage_usage - initial_storage_usage;
    println!("[request_review] Storage used: {} bytes", storage_used);

    return Ok(());
}