use near_api::NearToken;
use testresult::TestResult;

use crate::env::ft::{FT_STORAGE_DEPOSIT, FungibleToken};
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::env::{Env, INIT_LOCK};
use crate::tests::assertions::{assert_storage_balance, assert_transaction_failure_contains};
use crate::tests::{ONE_YOCTO, STAKE_AMOUNT, ZERO_AMOUNT, stake_message};

const NON_ZERO_BALANCE_UNREGISTER_ERROR: &str =
    "Can't unregister the account with the positive balance without force";

#[tokio::test]
async fn test_check_total_balance_after_storage_deposits() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    let balance_checker = async |contract: &crate::env::types::Contract| -> TestResult {
        assert_eq!(
            contract
                .near_balance()
                .await?
                .total
                .saturating_add(contract.near_balance().await?.locked),
            contract.get_total_balance().await?
        );

        Ok(())
    };

    env.lst.ft_storage_deposit(alice, alice.id()).await?;
    balance_checker(&env.lst).await?;

    env.lst
        .ft_storage_withdraw(alice, Some(ZERO_AMOUNT))
        .await?;
    balance_checker(&env.lst).await?;

    env.lst.ft_storage_withdraw(alice, None).await?;
    balance_checker(&env.lst).await?;

    env.lst.ft_storage_unregister(alice, None).await?;
    balance_checker(&env.lst).await?;

    env.lst.ft_storage_unregister(bob, Some(true)).await?;
    balance_checker(&env.lst).await?;

    Ok(())
}

#[tokio::test]
async fn test_storage_deposit_registers_new_account() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let initial_total_balance = env.lst.get_total_balance().await?;

    assert!(env.lst.ft_storage_balance_of(alice.id()).await?.is_none());

    env.lst.ft_storage_deposit(alice, alice.id()).await?;

    assert_storage_balance(
        env.lst.ft_storage_balance_of(alice.id()).await?,
        FT_STORAGE_DEPOSIT,
        ZERO_AMOUNT,
    );
    assert_eq!(
        env.lst.get_total_balance().await?,
        initial_total_balance.saturating_add(FT_STORAGE_DEPOSIT)
    );

    let alice_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_balance_after.total.saturating_add(FT_STORAGE_DEPOSIT),
        alice_balance_before.total
    );
    assert_eq!(alice_balance_after.locked, alice_balance_before.locked);

    Ok(())
}

#[tokio::test]
async fn test_storage_deposit_registration_only_registers_new_account() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let initial_total_balance = env.lst.get_total_balance().await?;
    let oversized_storage_deposit = FT_STORAGE_DEPOSIT.saturating_add(NearToken::from_near(1));

    assert!(env.lst.ft_storage_balance_of(alice.id()).await?.is_none());

    env.lst
        .ft_storage_deposit_with_amount(alice, alice.id(), oversized_storage_deposit, Some(true))
        .await?;

    assert_storage_balance(
        env.lst.ft_storage_balance_of(alice.id()).await?,
        FT_STORAGE_DEPOSIT,
        ZERO_AMOUNT,
    );
    assert_eq!(
        env.lst.get_total_balance().await?,
        initial_total_balance.saturating_add(FT_STORAGE_DEPOSIT)
    );

    let alice_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_balance_after.total.saturating_add(FT_STORAGE_DEPOSIT),
        alice_balance_before.total
    );

    Ok(())
}

#[tokio::test]
async fn test_storage_unregister_with_zero_balance_succeeds() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let initial_total_balance = env.lst.get_total_balance().await?;

    env.lst.ft_storage_deposit(alice, alice.id()).await?;

    assert_storage_balance(
        env.lst.ft_storage_balance_of(alice.id()).await?,
        FT_STORAGE_DEPOSIT,
        ZERO_AMOUNT,
    );

    env.lst.ft_storage_unregister(alice, None).await?;

    assert!(env.lst.ft_storage_balance_of(alice.id()).await?.is_none());
    assert_eq!(env.lst.get_total_balance().await?, initial_total_balance);
    assert_eq!(alice.near_balance().await?, alice_balance_before);

    Ok(())
}

#[tokio::test]
async fn test_storage_unregister_with_non_zero_balance_without_force_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&String>),
        )
        .await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);

    let result = env.lst.ft_storage_unregister(alice, None).await;
    assert_transaction_failure_contains(result, NON_ZERO_BALANCE_UNREGISTER_ERROR);

    assert_storage_balance(
        env.lst.ft_storage_balance_of(alice.id()).await?,
        FT_STORAGE_DEPOSIT,
        ZERO_AMOUNT,
    );
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );

    Ok(())
}

#[tokio::test]
async fn test_storage_unregister_with_non_zero_balance_and_force_burns_balance() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&String>),
        )
        .await?;

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );

    env.lst.ft_storage_unregister(alice, Some(true)).await?;

    assert!(env.lst.ft_storage_balance_of(alice.id()).await?.is_none());
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    let alice_balance_after = alice.near_balance().await?;
    // Force unregister burns Alice's LST balance, so she loses her staking position.
    // Only the storage deposit is returned.
    assert_eq!(
        alice_balance_after.total,
        alice_balance_before
            .total
            .saturating_sub(STAKE_AMOUNT)
            .saturating_add(FT_STORAGE_DEPOSIT)
    );

    Ok(())
}

#[tokio::test]
async fn test_storage_withdraw_without_available_balance_keeps_storage_deposit() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let oversized_storage_deposit = FT_STORAGE_DEPOSIT.saturating_add(NearToken::from_near(1));

    env.lst
        .ft_storage_deposit_with_amount(alice, alice.id(), oversized_storage_deposit, None)
        .await?;

    assert_storage_balance(
        env.lst.ft_storage_balance_of(alice.id()).await?,
        FT_STORAGE_DEPOSIT,
        ZERO_AMOUNT,
    );
    let alice_balance_after_deposit = alice.near_balance().await?;
    assert_eq!(
        alice_balance_after_deposit
            .total
            .saturating_add(FT_STORAGE_DEPOSIT),
        alice_balance_before.total
    );
    assert_eq!(
        alice_balance_after_deposit.locked,
        alice_balance_before.locked
    );

    env.lst.ft_storage_withdraw(alice, None).await?;

    assert_storage_balance(
        env.lst.ft_storage_balance_of(alice.id()).await?,
        FT_STORAGE_DEPOSIT,
        ZERO_AMOUNT,
    );

    let alice_balance_after_withdraw = alice.near_balance().await?;
    assert_eq!(
        alice_balance_after_withdraw
            .total
            .saturating_add(FT_STORAGE_DEPOSIT)
            .saturating_add(ONE_YOCTO),
        alice_balance_before.total
    );

    Ok(())
}

#[tokio::test]
async fn test_storage_balance_of_registered_and_unregistered_accounts() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let bob = env.bob();
    let alice_balance_before = alice.near_balance().await?;
    let bob_balance_before = bob.near_balance().await?;

    assert!(env.lst.ft_storage_balance_of(alice.id()).await?.is_none());
    assert!(env.lst.ft_storage_balance_of(bob.id()).await?.is_none());

    env.lst.ft_storage_deposit(alice, alice.id()).await?;

    assert_storage_balance(
        env.lst.ft_storage_balance_of(alice.id()).await?,
        FT_STORAGE_DEPOSIT,
        ZERO_AMOUNT,
    );
    assert!(env.lst.ft_storage_balance_of(bob.id()).await?.is_none());

    let alice_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_balance_after.total.saturating_add(FT_STORAGE_DEPOSIT),
        alice_balance_before.total
    );
    assert_eq!(bob.near_balance().await?, bob_balance_before);

    Ok(())
}
