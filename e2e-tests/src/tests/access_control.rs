use liquid_staking_token::pool::WithdrawTokens;
use near_api::types::transaction::result::ExecutionSuccess;
use near_api::{AccountId, Data, PublicKey};
use near_sdk::serde_json::json;
use testresult::TestResult;

use crate::env::ft::FungibleToken;
use crate::env::pool::StakingPool;
use crate::env::types::{Account, Contract};
use crate::env::{Env, INIT_LOCK, LST, validator_signer};
use crate::tests::assertions::assert_transaction_failure_contains;
use crate::tests::{STAKE_AMOUNT, ZERO_AMOUNT, stake_message, unstake_message};

const PAUSE_ALL_KEY: &str = "ALL";
const PAUSED_ERROR: &str = "Pausable: Method is paused";
const ACCESS_CONTROL_ERROR: &str = "Insufficient permissions";

async fn pause_feature(contract: &Contract, signer: &Account) -> anyhow::Result<ExecutionSuccess> {
    contract
        .inner
        .call_function("pa_pause_feature", json!({ "key": PAUSE_ALL_KEY }))
        .transaction()
        .max_gas()
        .with_signer(signer.id().clone(), signer.signer())
        .send_to(contract.config())
        .await?
        .into_result()
        .map_err(Into::into)
}

async fn unpause_feature(
    contract: &Contract,
    signer: &Account,
) -> anyhow::Result<ExecutionSuccess> {
    contract
        .inner
        .call_function("pa_unpause_feature", json!({ "key": PAUSE_ALL_KEY }))
        .transaction()
        .max_gas()
        .with_signer(signer.id().clone(), signer.signer())
        .send_to(contract.config())
        .await?
        .into_result()
        .map_err(Into::into)
}

async fn is_paused(contract: &Contract) -> anyhow::Result<bool> {
    contract
        .inner
        .call_function("pa_is_paused", json!({ "key": PAUSE_ALL_KEY }))
        .read_only()
        .fetch_from(contract.config())
        .await
        .map(|result: Data<bool>| result.data)
        .map_err(Into::into)
}

async fn get_owner_id(contract: &Contract) -> anyhow::Result<AccountId> {
    contract
        .inner
        .call_function("get_owner_id", json!({}))
        .read_only()
        .fetch_from(contract.config())
        .await
        .map(|result: Data<AccountId>| result.data)
        .map_err(Into::into)
}

async fn get_staking_key(contract: &Contract) -> anyhow::Result<PublicKey> {
    contract
        .inner
        .call_function("get_staking_key", json!({}))
        .read_only()
        .fetch_from(contract.config())
        .await
        .map(|result: Data<PublicKey>| result.data)
        .map_err(Into::into)
}

#[tokio::test]
async fn test_pause_contract_stake_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let owner = env.lst.as_account();

    pause_feature(&env.lst, &owner).await?;

    assert!(is_paused(&env.lst).await?);

    let result = env
        .lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await;
    assert_transaction_failure_contains(result, PAUSED_ERROR);

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    Ok(())
}

#[tokio::test]
async fn test_pause_contract_withdraw_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;

    let unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &unstake_message)
        .await?;

    env.wait_unstake_cooldown().await?;

    let owner = env.lst.as_account();
    pause_feature(&env.lst, &owner).await?;

    assert!(is_paused(&env.lst).await?);

    let result = env.lst.withdraw(alice, &unstake_message).await;
    assert_transaction_failure_contains(result, PAUSED_ERROR);

    assert_eq!(env.lst.get_total_pending_withdrawals().await?, STAKE_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_unpause_contract_operations_resume() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let owner = env.lst.as_account();

    // Set up a pending withdrawal so we can verify both `stake` and `withdraw`
    // resume after unpause within the same test (self-contained flow).
    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;

    let unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &unstake_message)
        .await?;

    env.wait_unstake_cooldown().await?;

    assert_eq!(env.lst.get_total_pending_withdrawals().await?, STAKE_AMOUNT);
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);

    // Pause and assert that both `stake` and `withdraw` are blocked.
    pause_feature(&env.lst, &owner).await?;
    assert!(is_paused(&env.lst).await?);

    let stake_while_paused = env
        .lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await;
    assert_transaction_failure_contains(stake_while_paused, PAUSED_ERROR);

    let withdraw_while_paused = env.lst.withdraw(alice, &unstake_message).await;
    assert_transaction_failure_contains(withdraw_while_paused, PAUSED_ERROR);

    // Side effects must remain unchanged while paused.
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, STAKE_AMOUNT);
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);

    // Unpause and verify both operations succeed.
    unpause_feature(&env.lst, &owner).await?;
    assert!(!is_paused(&env.lst).await?);

    env.lst.withdraw(alice, &unstake_message).await?;
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);

    let supply_before_second_stake = env.lst.ft_total_supply().await?;
    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;

    // The second stake happens after a full cooldown, so validator rewards
    // have already shifted the LST/NEAR exchange rate. We can't assert a
    // 1:1 mint anymore, only that the stake produced LST and the supply
    // grew internally consistently with alice's balance.
    let alice_balance_after = env.lst.ft_balance_of(alice.id()).await?;
    assert!(
        alice_balance_after > ZERO_AMOUNT,
        "Expected alice to receive LST after the resumed stake, got 0",
    );
    assert_eq!(
        env.lst.ft_total_supply().await?,
        supply_before_second_stake.saturating_add(alice_balance_after),
    );

    Ok(())
}

#[tokio::test]
async fn test_non_owner_pause_contract_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    let result = pause_feature(&env.lst, alice).await;
    assert_transaction_failure_contains(result, ACCESS_CONTROL_ERROR);

    assert!(!is_paused(&env.lst).await?);

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_get_owner_id_returns_correct_owner() -> TestResult {
    let env = Env::builder().build().await?;

    // Compare against the configured owner literal (`LST`), independently of
    // `env.lst.id()` — owner equality is a contract invariant, not a property
    // derived from the contract's own account id.
    let expected_owner: AccountId = LST.parse()?;
    assert_eq!(get_owner_id(&env.lst).await?, expected_owner);

    Ok(())
}

#[tokio::test]
async fn test_get_staking_key_returns_correct_validator_key() -> TestResult {
    let env = Env::builder().build().await?;
    let validator_public_key = validator_signer().get_public_key().await?;

    assert_eq!(get_staking_key(&env.lst).await?, validator_public_key);

    Ok(())
}
