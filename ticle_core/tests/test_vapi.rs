use near_sdk::{json_types::U128, NearToken};
use serde_json::json;

use crate::common::utils::*;
pub mod common;

#[tokio::test]
async fn test_vapi() -> anyhow::Result<()> {
    let initial_balance = U128::from(NearToken::from_near(10000).as_yoctonear());
    let worker = near_workspaces::sandbox().await?;
    let (ft_contract, owner, core_contract) = init(&worker, initial_balance).await?;

    register_user(&ft_contract, core_contract.id()).await?;

    let users = create_users(&worker, vec!["alice", "bob"], vec![10, 10]).await?;
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

    let vapi_id = "test-vapi";
    let res = alice
        .call(core_contract.id(), "create_vapi")
        .args_json(json!({"vapi_id": vapi_id}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    let transfer_balance = U128::from(NearToken::from_near(10).as_yoctonear());

    // alice가 10토큰을 VAPI에 입금
    let res = alice
        .call(ft_contract.id(), "ft_transfer_call")
        .args_json((core_contract.id(), transfer_balance, Option::<String>::None, serde_json::json!({ "vapi_id": vapi_id }).to_string()))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;

    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    // bob이 10토큰을 VAPI에 입금
    let res = bob
        .call(ft_contract.id(), "ft_transfer_call")
        .args_json((core_contract.id(), transfer_balance, Option::<String>::None, serde_json::json!({ "vapi_id": vapi_id }).to_string()))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;

    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    // 10토큰 정산
    let res = owner
        .call(ft_contract.id(), "ft_transfer_call")
        .args_json((core_contract.id(), transfer_balance, Option::<String>::None, serde_json::json!({ "vapi_ids": vec![vapi_id], "amounts": vec![transfer_balance] }).to_string()))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;

    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    // 정산 금액: 10, delegator 사용료: 39%, 스테이킹 인원 수: 2명
    // pending reward = (10 * 39 / 100) / 2 = 1.95
    let alice_reward = core_contract
        .call("pending_reward")
        .args_json((alice.id(), vapi_id))
        .view()
        .await?
        .json::<u128>()?;
    println!("alice_reward: {}", alice_reward);
    assert_eq!(alice_reward, 19_500_000_000_000_000_000_000_00);

    let bob_reward = core_contract
        .call("pending_reward")
        .args_json((bob.id(), vapi_id))
        .view()
        .await?
        .json::<u128>()?;
    assert_eq!(bob_reward, 19_500_000_000_000_000_000_000_00);

    // alice는 추가로 10토큰을 입금
    // 이 과정에서 클레임하지 않은 토큰(1.95)이 alice에게 전송됨
    let alice_origin_balance = ft_contract
        .call("ft_balance_of")
        .args_json(json!({"account_id": alice.id()}))
        .view()
        .await?
        .json::<U128>()?
        .0;
    println!("alice_origin_balance: {}", alice_origin_balance);
    
    let res = alice.call(ft_contract.id(), "ft_transfer_call")
        .args_json((core_contract.id(), transfer_balance, Option::<String>::None, serde_json::json!({ "vapi_id": vapi_id }).to_string()))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;
    assert!(res.is_success());
    
    let alice_after_balance = ft_contract
        .call("ft_balance_of")
        .args_json(json!({"account_id": alice.id()}))
        .view()
        .await?
        .json::<U128>()?
        .0;
    // alice의 원금에서 추가로 입금한 10토큰을 빼고 클레임한 1.95토큰을 더한 금액
    assert_eq!(alice_after_balance, alice_origin_balance - transfer_balance.0 + 19_500_000_000_000_000_000_000_00);

    // 10토큰 정산
    let res = owner
        .call(ft_contract.id(), "ft_transfer_call")
        .args_json((core_contract.id(), transfer_balance, Option::<String>::None, serde_json::json!({ "vapi_ids": vec![vapi_id], "amounts": vec![transfer_balance] }).to_string()))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;

    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    // alice가 클레임 가능한 금액: 2.6
    let alice_reward = core_contract
        .call("pending_reward")
        .args_json((alice.id(), vapi_id))
        .view()
        .await?
        .json::<u128>()?;
    assert_eq!(alice_reward, 26_000_000_000_000_000_000_000_00);
    
    // bob가 클레임 가능한 금액: 3.25 (이전 1.95 + 추가 1.3)
    let bob_reward = core_contract
        .call("pending_reward")
        .args_json((bob.id(), vapi_id))
        .view()
        .await?
        .json::<u128>()?;
    assert_eq!(bob_reward, 32_500_000_000_000_000_000_000_00);

    return Ok(());
}