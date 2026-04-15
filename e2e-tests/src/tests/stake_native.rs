use near_api::NearToken;
use testresult::TestResult;

use crate::env::ft::FT_STORAGE_DEPOSIT;
use crate::env::pool::StakingPool;
use crate::env::{Env, INIT_LOCK, ft::FungibleToken, mt::MultiToken, native::Native};
use crate::tests::{STAKE_AMOUNT, ZERO_AMOUNT, stake_message};

#[tokio::test]
async fn test_stake_with_native_near_and_get_on_intents() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(env.defuse.id(), None, Some(alice.id())),
        )
        .await?;

    assert_eq!(
        env.lst.near_balance().await?.locked,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(env.lst.ft_balance_of(env.defuse.id()).await?, STAKE_AMOUNT);
    assert_eq!(env.lst.ft_total_supply().await?, STAKE_AMOUNT);

    let staked_tokens = env.defuse.mt_balance_of(alice.id(), env.lst.id()).await?;
    assert_eq!(staked_tokens, STAKE_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice_native_balance_after.total),
        STAKE_AMOUNT
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_native_near_and_get_on_intents_bob() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    let alice_native_balance_before = alice.near_balance().await?;
    let bob_native_balance_before = bob.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(env.defuse.id(), None, Some(bob.id())),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let intents_balance = env.lst.ft_balance_of(env.defuse.id()).await?;
    assert_eq!(intents_balance, STAKE_AMOUNT);
    let total_supply = env.lst.ft_total_supply().await?;
    assert_eq!(total_supply, STAKE_AMOUNT);

    let alice_intents_balance = env.defuse.mt_balance_of(alice.id(), env.lst.id()).await?;
    assert_eq!(alice_intents_balance, ZERO_AMOUNT);
    let bob_intents_balance = env.defuse.mt_balance_of(bob.id(), env.lst.id()).await?;
    assert_eq!(bob_intents_balance, STAKE_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice_native_balance_after.total),
        STAKE_AMOUNT
    );
    let bob_native_balance_after = bob.near_balance().await?;
    assert_eq!(bob_native_balance_before, bob_native_balance_after);

    Ok(())
}

#[tokio::test]
async fn test_stake_with_native_near_and_get_on_nep141() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let intents_lst_balance = env.lst.ft_balance_of(env.defuse.id()).await?;
    assert_eq!(intents_lst_balance, ZERO_AMOUNT);
    let total_lst_supply = env.lst.ft_total_supply().await?;
    assert_eq!(total_lst_supply, STAKE_AMOUNT);

    let alice_lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(alice_lst_balance, STAKE_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice_native_balance_after.total),
        STAKE_AMOUNT
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_native_near_and_get_on_nep141_to_bob() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();
    let alice_native_balance_before = alice.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(bob.id(), None, None::<&str>),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let intents_lst_balance = env.lst.ft_balance_of(env.defuse.id()).await?;
    assert_eq!(intents_lst_balance, ZERO_AMOUNT);
    let total_lst_supply = env.lst.ft_total_supply().await?;
    assert_eq!(total_lst_supply, STAKE_AMOUNT);

    let alice_lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(alice_lst_balance, ZERO_AMOUNT);

    let bob_lst_balance = env.lst.ft_balance_of(bob.id()).await?;
    assert_eq!(bob_lst_balance, STAKE_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice_native_balance_after.total),
        STAKE_AMOUNT
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_native_near_and_get_on_nep141_without_registration() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK);

    let intents_lst_balance = env.lst.ft_balance_of(env.defuse.id()).await?;
    assert_eq!(intents_lst_balance, ZERO_AMOUNT);
    let total_lst_supply = env.lst.ft_total_supply().await?;
    assert_eq!(total_lst_supply, ZERO_AMOUNT);

    let alice_lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(alice_lst_balance, ZERO_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(alice_native_balance_before, alice_native_balance_after);

    Ok(())
}

#[tokio::test]
async fn test_stake_with_native_near_and_get_on_nep141_with_registration() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT.saturating_add(FT_STORAGE_DEPOSIT),
            stake_message(alice.id(), Some(FT_STORAGE_DEPOSIT), None::<&str>),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let intents_lst_balance = env.lst.ft_balance_of(env.defuse.id()).await?;
    assert_eq!(intents_lst_balance, ZERO_AMOUNT);
    let total_lst_supply = env.lst.ft_total_supply().await?;
    assert_eq!(total_lst_supply, STAKE_AMOUNT);

    let alice_lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(alice_lst_balance, STAKE_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before.total,
        alice_native_balance_after
            .total
            .saturating_add(STAKE_AMOUNT)
            .saturating_add(FT_STORAGE_DEPOSIT)
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_storage_deposit_exceeding_amount_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    // Attach 1 NEAR but request a 2 NEAR storage deposit — contract must panic.
    let deposit = NearToken::from_near(1);
    let oversized_storage_deposit = NearToken::from_near(2);

    let result = env
        .lst
        .stake(
            alice,
            deposit,
            stake_message(alice.id(), Some(oversized_storage_deposit), None::<&str>),
        )
        .await;

    assert!(
        result.is_err(),
        "Expected stake to fail when storage_deposit exceeds the attached amount"
    );

    // No tokens minted, locked balance unchanged.
    assert_eq!(env.lst.ft_total_supply().await?, ZERO_AMOUNT);
    assert_eq!(env.lst.near_balance().await?.locked, INIT_LOCK);

    Ok(())
}
