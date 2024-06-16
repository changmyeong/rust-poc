use std::str::FromStr;

use near_sdk::{json_types::U128, NearToken};
use serde_json::json;
use near_crypto::SecretKey;

use crate::common::utils::*;
pub mod common;

#[tokio::test]
async fn test_review() -> anyhow::Result<()> {
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

    // 1. alice가 VAPI를 생성한다.
    let vapi_id = "alice-vapi";
    let res = alice
        .call(core_contract.id(), "create_vapi")
        .args_json(json!({"vapi_id": vapi_id}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    // 2. 오프체인에서 생성된 VAPI에 대해 코드리뷰를 신청한다.

    // 3. bob, charlie는 코드리뷰에 참여하겠다고 제안한다.

    // 4. 오프체인 서버에서 ed25519 알고리즘으로 서명을 생성한다.
    let amount = U128::from(NearToken::from_near(10).as_yoctonear());
    let vapi_version = "1.0";
    let message = format!("{},{},{:?},{:?}", vapi_id, vapi_version, vec![bob.id(), charlie.id()], vec![amount, amount]);
    let message_bytes = message.as_bytes();
    
    let owner_secret_key = SecretKey::from_str(&owner.secret_key().to_string()).unwrap();

    // ex) ed25519:4FpUmQQstsqxH2gnwTajRyizbkanBqSkiBtX3g9wY5ngwFGfQPe5utw6EhwHVjx9sB7wY4F6S6TwReTCFR9u5ZtY
    let signature = owner_secret_key.sign(message_bytes).to_string();
    
    // 5. alice는 서버의 서명과 함께 트랜잭션을 전송한다.
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
    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    // 6. 리뷰어들은 2주가 지나지 않으면 리워드 수령이 불가능하다.
    let res = bob
        .call(core_contract.id(), "claim_review_reward")
        .args_json(json!({"vapi_id": vapi_id}))
        .max_gas()
        .transact()
        .await?;
    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_failure());

    // 7. 2주가 지난 뒤 리뷰어들이 리워드 수령이 가능하다.
    // 재단에 fast_forward 문의 필요.
    // https://docs.near.org/sdk/rust/testing/integration-tests#fast-forwarding---fast-forward-to-a-future-block
    const TWO_WEEKS: u64 = 60 * 60 * 24 * 14;
    worker.fast_forward(TWO_WEEKS).await?;

    let res = bob
        .call(core_contract.id(), "claim_review_reward")
        .args_json(json!({"vapi_id": vapi_id}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    return Ok(());
}

// 가스비 시뮬레이션을 할 수 있는 테스트 코드를 작성한다!