use near_sdk::json_types::U128;
use near_workspaces::{types::NearToken, Account, AccountId, Contract, DevNetwork, Worker};
use near_contract_standards::fungible_token::metadata::FungibleTokenMetadata;
use serde_json::json;

const ONE_YOCTO: NearToken = NearToken::from_yoctonear(1);

async fn register_user(contract: &Contract, account_id: &AccountId) -> anyhow::Result<()> {
    let res = contract
        .call("storage_deposit")
        .args_json((account_id, Option::<bool>::None))
        .max_gas()
        .deposit(near_sdk::env::storage_byte_cost().saturating_mul(125))
        .transact()
        .await?;
    assert!(res.is_success());

    return Ok(());
}

async fn create_users(worker: &Worker<impl DevNetwork>, users: Vec<&str>, nears: Vec<u128>) -> anyhow::Result<Vec<Account>> {
    let mut accounts = Vec::new();
    let account = worker.dev_create_account().await?;
    for (user, near) in users.iter().zip(nears.iter()) {
        let account = account
            .create_subaccount(user)
            .initial_balance(NearToken::from_near(*near))
            .transact()
            .await?;
        accounts.push(account.into_result()?);
    }
    return Ok(accounts);
}

async fn init(
    worker: &Worker<impl DevNetwork>,
    initial_balance: U128
) -> anyhow::Result<(Contract, Account, Contract)> {
    let token_wasm = include_bytes!("../../target/wasm32-unknown-unknown/release/token.wasm");
    let ft_contract = worker.dev_deploy(token_wasm).await?;

    let token_metadata = FungibleTokenMetadata {
        spec: "ft-1.0.0".to_string(),
        name: "T Token".to_string(),
        symbol: "TIC".to_string(),
        icon: None,
        reference: None,
        reference_hash: None,
        decimals: 24,
    };

    let res = ft_contract
        .call("new")
        .args_json((ft_contract.id(), initial_balance, token_metadata))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    let users = create_users(worker, vec!["owner"], vec![50]).await?;

    let owner = users.get(0).unwrap().clone();
    register_user(&ft_contract, owner.id()).await?;
    
    let res = ft_contract
        .call("ft_transfer")
        .args_json((owner.id(), initial_balance, Option::<String>::None))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;
    assert!(res.is_success());

    let pool_wasm = include_bytes!("../../target/wasm32-unknown-unknown/release/pool.wasm");
    let pool_contract = worker.dev_deploy(pool_wasm).await?;

    let res = pool_contract
        .call("new")
        .args_json((ft_contract.id(), owner.id()))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    return Ok((ft_contract, owner, pool_contract));
}

#[tokio::test]
async fn test_with_ft_contract() -> anyhow::Result<()> {
    let initial_balance = U128::from(NearToken::from_near(10000).as_yoctonear());
    let worker = near_workspaces::sandbox().await?;
    let (ft_contract, owner, pool_contract) = init(&worker, initial_balance).await?;

    register_user(&ft_contract, pool_contract.id()).await?;

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

    // HACK: 파라미터 받고 args_json, args_borsh로 넘기면 json 파싱 실패로 에러가 발생함.
    //// 테스트 했던 방법들
    //// 1. args_json & view()로는 호출이 됨
    //// 2. #[near(serializers=[borsh, json])] 넣고 호출해도 에러 발생
    let res = pool_contract
        .call("create_sample_vapi")
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    let transfer_balance = U128::from(NearToken::from_near(10).as_yoctonear());
    let res = alice
        .call(ft_contract.id(), "ft_transfer_call")
        .args_json((pool_contract.id(), transfer_balance, Option::<String>::None, serde_json::json!({ "vapi_id": "test-vapi" }).to_string()))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;

    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    worker.fast_forward(4).await?;

    let res = bob
        .call(ft_contract.id(), "ft_transfer_call")
        .args_json((pool_contract.id(), transfer_balance, Option::<String>::None, serde_json::json!({ "vapi_id": "test-vapi" }).to_string()))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;

    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    worker.fast_forward(5).await?;

    let res = owner
        .call(ft_contract.id(), "ft_transfer_call")
        .args_json((pool_contract.id(), transfer_balance, Option::<String>::None, serde_json::json!({ "vapi_id": "test-vapi" }).to_string()))
        .max_gas()
        .deposit(ONE_YOCTO)
        .transact()
        .await?;

    res.logs().iter().for_each(|log| println!("{:?}", log));
    assert!(res.is_success());

    let res = pool_contract.call("pending_reward").args_json((alice.id(), "test-vapi")).view().await?.json::<u128>()?;
    println!("pending_reward(alice): {:?}", res);

    let res = pool_contract.call("pending_reward").args_json((bob.id(), "test-vapi")).view().await?.json::<u128>()?;
    println!("pending_reward(bob): {:?}", res);
    Ok(())
}