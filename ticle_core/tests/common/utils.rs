use near_sdk::json_types::U128;
use near_workspaces::{types::NearToken, Account, AccountId, Contract, DevNetwork, Worker};
use near_contract_standards::fungible_token::metadata::FungibleTokenMetadata;

pub const ONE_YOCTO: NearToken = NearToken::from_yoctonear(1);

pub async fn register_user(contract: &Contract, account_id: &AccountId) -> anyhow::Result<()> {
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

pub async fn create_users(worker: &Worker<impl DevNetwork>, users: Vec<&str>, nears: Vec<u128>) -> anyhow::Result<Vec<Account>> {
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

pub async fn init(
    worker: &Worker<impl DevNetwork>,
    initial_balance: U128
) -> anyhow::Result<(Contract, Account, Contract)> {
    let token_wasm = include_bytes!("../../../target/wasm32-unknown-unknown/release/token.wasm");
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

    let core_wasm = include_bytes!("../../../target/wasm32-unknown-unknown/release/ticle_core.wasm");
    let core_contract = worker.dev_deploy(core_wasm).await?;

    let res = owner
        .call(core_contract.id(), "new")
        .args_json((ft_contract.id(), owner.id()))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    return Ok((ft_contract, owner, core_contract));
}