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

    env.fast_forward(1).await?;

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
