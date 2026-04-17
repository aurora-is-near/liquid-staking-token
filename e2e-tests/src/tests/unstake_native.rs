use liquid_staking_token::pool::WithdrawTokens;
use near_api::NearToken;
use testresult::TestResult;

use crate::env::defuse::{Defuse, DefuseSigner};
use crate::env::ft::FungibleToken;
use crate::env::mt::MultiToken;
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::env::{Env, INIT_LOCK, INITIAL_BALANCE};
use crate::tests::{ONE_YOCTO, STAKE_AMOUNT, ZERO_AMOUNT, stake_message, unstake_message};

#[tokio::test]
async fn test_withdraw_before_cooldown_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;

    let unstake_msg = unstake_message(alice.id(), WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &unstake_msg)
        .await?;

    // Cooldown not elapsed — withdrawal must fail.
    let result = env.lst.withdraw(alice, &unstake_msg).await;
    assert!(
        result.is_err(),
        "Expected withdrawal to fail before cooldown"
    );

    // The unstake queue entry is still intact; waiting and retrying must succeed.
    env.wait_unstake_cooldown().await?;
    env.lst.withdraw(alice, &unstake_msg).await?;

    assert_eq!(
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(NearToken::from_yoctonear(2)), // add_public_key + ft_transfer_call
        INITIAL_BALANCE
    );

    Ok(())
}

#[tokio::test]
async fn test_withdraw_nonexistent_stake_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    // Alice never staked or unstaked, so the queue has no matching entry.
    let unstake_msg = unstake_message(alice.id(), WithdrawTokens::Native);
    let result = env.lst.withdraw(alice, &unstake_msg).await;
    assert!(
        result.is_err(),
        "Expected withdrawal to fail when no matching unstake entry exists"
    );

    // State unchanged.
    assert_eq!(env.lst.ft_total_supply().await?, ZERO_AMOUNT);
    assert_eq!(env.lst.near_balance().await?.locked, INIT_LOCK);

    Ok(())
}

#[tokio::test]
async fn test_unstake_native_by_withdrawing_lst_from_intents() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(env.defuse.id(), None, Some(alice.id())),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let intents_lst_balance = env.defuse.mt_balance_of(alice.id(), env.lst.id()).await?;
    assert_eq!(intents_lst_balance, STAKE_AMOUNT);

    let unstake_message = unstake_message(alice.id(), WithdrawTokens::Native);
    let withdraw_intent = alice
        .sign_withdraw_intent(
            env.defuse.id(),
            env.lst.id(),
            env.lst.id(),
            STAKE_AMOUNT,
            Some(unstake_message.clone()),
        )
        .await;

    env.defuse
        .execute_intents(alice.id(), vec![withdraw_intent])
        .await?;

    let total_supply = env.lst.ft_total_supply().await?;
    assert_eq!(total_supply, ZERO_AMOUNT);

    env.wait_unstake_cooldown().await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked.as_near(), INIT_LOCK.as_near());

    env.lst.withdraw(alice, &unstake_message).await?;

    assert_eq!(
        alice.near_balance().await?.total.saturating_add(ONE_YOCTO),
        INITIAL_BALANCE
    );

    Ok(())
}

#[tokio::test]
async fn test_unstake_native_by_sending_lst_back() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&String>),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(lst_balance, STAKE_AMOUNT);

    let unstake_message = unstake_message(alice.id(), WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &unstake_message)
        .await?;

    let total_supply = env.lst.ft_total_supply().await?;
    assert_eq!(total_supply, ZERO_AMOUNT);

    env.wait_unstake_cooldown().await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked.as_near(), INIT_LOCK.as_near());

    env.lst.withdraw(alice, &unstake_message).await?;

    assert_eq!(
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(NearToken::from_yoctonear(2)), // add_public_key + ft_transfer_call
        INITIAL_BALANCE
    );

    Ok(())
}

#[tokio::test]
async fn test_partial_unstake_preserves_remaining_lst() -> TestResult {
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

    let partial = STAKE_AMOUNT.saturating_div(4); // unstake 25%
    let remaining = STAKE_AMOUNT.saturating_sub(partial);

    let unstake_msg = unstake_message(alice.id(), WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), partial, &unstake_msg)
        .await?;

    // 75% of LST must remain with alice; total supply tracks it.
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, remaining);
    assert_eq!(env.lst.ft_total_supply().await?, remaining);

    env.wait_unstake_cooldown().await?;

    // Locked balance reflects only the staked portion.
    assert_eq!(
        env.lst.near_balance().await?.locked,
        INIT_LOCK.saturating_add(remaining)
    );

    env.lst.withdraw(alice, &unstake_msg).await?;

    // After withdrawal the 75% of LST is untouched.
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, remaining);
    assert_eq!(env.lst.ft_total_supply().await?, remaining);

    Ok(())
}

#[tokio::test]
async fn test_two_unstakes_to_native_by_sending_lst_from_wnear() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&String>),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(lst_balance, STAKE_AMOUNT);

    let half_stake_amount = STAKE_AMOUNT.saturating_div(2);

    let unstake_message = unstake_message(alice.id(), WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), half_stake_amount, &unstake_message)
        .await?;
    env.lst
        .ft_transfer_call(alice, env.lst.id(), half_stake_amount, &unstake_message)
        .await?;

    let lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(lst_balance, ZERO_AMOUNT);

    let total_supply = env.lst.ft_total_supply().await?;
    assert_eq!(total_supply, ZERO_AMOUNT);

    env.wait_unstake_cooldown().await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked.as_near(), INIT_LOCK.as_near());

    env.lst.withdraw(alice, &unstake_message).await?;

    let wnear_defuse_balance = env.wnear.ft_balance_of(env.defuse.id()).await?;
    assert_eq!(wnear_defuse_balance, ZERO_AMOUNT);

    assert_eq!(
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(NearToken::from_yoctonear(3)), // add_public_key + ft_transfer_call + ft_transfer_call
        INITIAL_BALANCE
    );
    let intents_balance = env.defuse.mt_balance_of(alice.id(), env.wnear.id()).await?;
    assert_eq!(intents_balance, ZERO_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_stake_native_near_by_itself_and_unstake_native_to_itself() -> TestResult {
    let env = Env::builder().build().await?;
    let lst_init_balance = env.lst.near_balance().await?;

    env.lst
        .stake(
            &env.lst.as_account(),
            STAKE_AMOUNT,
            stake_message(env.lst.id(), None, None::<&String>),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let lst_balance = env.lst.ft_balance_of(env.lst.id()).await?;
    assert_eq!(lst_balance, STAKE_AMOUNT);

    let unstake_message = unstake_message(env.lst.id(), WithdrawTokens::Native);

    env.lst
        .ft_on_transfer(
            &env.lst.as_account(),
            env.lst.id(),
            STAKE_AMOUNT,
            &unstake_message,
        )
        .await?;

    let lst_balance = env.lst.ft_balance_of(env.lst.id()).await?;
    assert_eq!(lst_balance, ZERO_AMOUNT);

    let total_supply = env.lst.ft_total_supply().await?;
    assert_eq!(total_supply, ZERO_AMOUNT);

    env.wait_unstake_cooldown().await?;

    env.lst
        .withdraw(&env.lst.as_account(), &unstake_message)
        .await?;

    assert_eq!(env.lst.near_balance().await?, lst_init_balance);

    Ok(())
}

#[tokio::test]
async fn test_stake_native_near_by_itself_and_unstake_native_to_alice() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    let alice_init_balance = alice.near_balance().await?;
    let lst_init_balance = env.lst.near_balance().await?;

    env.lst
        .stake(
            &env.lst.as_account(),
            STAKE_AMOUNT,
            stake_message(env.lst.id(), None, None::<&String>),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let lst_balance = env.lst.ft_balance_of(env.lst.id()).await?;
    assert_eq!(lst_balance, STAKE_AMOUNT);

    let unstake_message = unstake_message(alice.id(), WithdrawTokens::Native);

    env.lst
        .ft_on_transfer(
            &env.lst.as_account(),
            env.lst.id(),
            STAKE_AMOUNT,
            &unstake_message,
        )
        .await?;

    let lst_balance = env.lst.ft_balance_of(env.lst.id()).await?;
    assert_eq!(lst_balance, ZERO_AMOUNT);

    let total_supply = env.lst.ft_total_supply().await?;
    assert_eq!(total_supply, ZERO_AMOUNT);

    env.wait_unstake_cooldown().await?;

    env.lst
        .withdraw(&env.lst.as_account(), &unstake_message)
        .await?;

    assert_eq!(
        env.lst.near_balance().await?.total,
        lst_init_balance.total.saturating_sub(STAKE_AMOUNT)
    );

    assert_eq!(
        alice.near_balance().await?.total,
        alice_init_balance.total.saturating_add(STAKE_AMOUNT)
    );

    Ok(())
}
