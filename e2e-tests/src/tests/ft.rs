use liquid_staking_token::pool::WithdrawTokens;
use near_api::Data;
use near_sdk::serde_json::{Value, json};
use testresult::TestResult;

use crate::env::ft::FungibleToken;
use crate::env::pool::StakingPool;
use crate::env::{Env, INIT_LOCK};
use crate::tests::assertions::assert_transaction_failure_contains;
use crate::tests::{STAKE_AMOUNT, ZERO_AMOUNT, stake_message, unstake_message};

const ACCOUNT_NOT_REGISTERED_ERROR: &str = "is not registered";

#[tokio::test]
async fn test_ft_transfer_from_alice_to_bob() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();
    let transfer_amount = STAKE_AMOUNT.saturating_div(2);
    let expected_alice_balance = STAKE_AMOUNT.saturating_sub(transfer_amount);

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;

    env.lst
        .ft_transfer(alice, bob.id(), transfer_amount)
        .await?;

    assert_eq!(
        env.lst.ft_balance_of(alice.id()).await?,
        expected_alice_balance
    );
    assert_eq!(env.lst.ft_balance_of(bob.id()).await?, transfer_amount);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );

    Ok(())
}

#[tokio::test]
async fn test_ft_transfer_to_unregistered_account_fails() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let unregistered_account = "test.near".parse()?;

    env.lst.ft_storage_deposit(alice, alice.id()).await?;
    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;

    let result = env
        .lst
        .ft_transfer(alice, &unregistered_account, STAKE_AMOUNT)
        .await;
    assert_transaction_failure_contains(result, ACCOUNT_NOT_REGISTERED_ERROR);

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );

    Ok(())
}

#[tokio::test]
async fn test_ft_transfer_call_to_contract_consumes_tokens() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let ft_receiver = env.deploy_ft_receiver().await?;
    let transfer_amount = STAKE_AMOUNT.saturating_div(2);
    let expected_alice_balance = STAKE_AMOUNT.saturating_sub(transfer_amount);

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;

    env.lst
        .ft_transfer_call(alice, ft_receiver.id(), transfer_amount, ZERO_AMOUNT)
        .await?;

    assert_eq!(
        env.lst.ft_balance_of(alice.id()).await?,
        expected_alice_balance
    );
    assert_eq!(
        env.lst.ft_balance_of(ft_receiver.id()).await?,
        transfer_amount
    );
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );

    Ok(())
}

#[tokio::test]
async fn test_ft_transfer_call_to_contract_returns_full_refund() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let ft_receiver = env.deploy_ft_receiver().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;

    env.lst
        .ft_transfer_call(alice, ft_receiver.id(), STAKE_AMOUNT, STAKE_AMOUNT)
        .await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(env.lst.ft_balance_of(ft_receiver.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );

    Ok(())
}

#[tokio::test]
async fn test_ft_total_supply_reflects_stakes_transfers_and_burns() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();
    let bob_stake = STAKE_AMOUNT.saturating_div(2);
    let transfer_amount = STAKE_AMOUNT.saturating_div(4);
    let unstake_amount = STAKE_AMOUNT.saturating_div(2);
    let expected_supply_after_stakes = INIT_LOCK
        .saturating_add(STAKE_AMOUNT)
        .saturating_add(bob_stake);

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;
    env.lst
        .stake(bob, bob_stake, stake_message(bob.id(), false, None::<&str>))
        .await?;

    assert_eq!(
        env.lst.ft_total_supply().await?,
        expected_supply_after_stakes
    );

    env.lst
        .ft_transfer(alice, bob.id(), transfer_amount)
        .await?;

    assert_eq!(
        env.lst.ft_balance_of(alice.id()).await?,
        STAKE_AMOUNT.saturating_sub(transfer_amount)
    );
    assert_eq!(
        env.lst.ft_balance_of(bob.id()).await?,
        bob_stake.saturating_add(transfer_amount)
    );
    assert_eq!(
        env.lst.ft_total_supply().await?,
        expected_supply_after_stakes
    );

    let unstake_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), unstake_amount, &unstake_message)
        .await?;

    assert_eq!(
        env.lst.ft_total_supply().await?,
        expected_supply_after_stakes.saturating_sub(unstake_amount)
    );
    assert_eq!(
        env.lst.get_total_pending_withdrawals().await?,
        unstake_amount
    );

    Ok(())
}

#[tokio::test]
async fn test_ft_metadata_returns_expected_fields() -> TestResult {
    let env = Env::builder().build().await?;

    let metadata: Data<Value> = env
        .lst
        .inner
        .call_function("ft_metadata", json!({}))
        .read_only()
        .fetch_from(env.lst.config())
        .await?;

    assert_eq!(metadata.data["spec"].as_str(), Some("ft-1.0.0"));
    assert_eq!(metadata.data["name"].as_str(), Some("stNEAR"));
    assert_eq!(metadata.data["symbol"].as_str(), Some("stNEAR"));
    assert_eq!(metadata.data["decimals"].as_u64(), Some(24));

    Ok(())
}
