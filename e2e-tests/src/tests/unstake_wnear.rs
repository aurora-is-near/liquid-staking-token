use liquid_staking_token::pool::WithdrawTokens;
use testresult::TestResult;

use crate::env::defuse::{Defuse, DefuseSigner};
use crate::env::ft::{FT_STORAGE_DEPOSIT, FungibleToken};
use crate::env::mt::MultiToken;
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::env::wnear::WNear;
use crate::env::{Env, INIT_LOCK, INITIAL_BALANCE};
use crate::tests::{ONE_YOCTO, STAKE_AMOUNT, ZERO_AMOUNT, stake_message, unstake_message};

#[tokio::test]
async fn test_unstake_by_withdrawing_lst_from_intents() -> TestResult {
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

    let unstake_message = unstake_message(
        env.defuse.id(),
        WithdrawTokens::Wnear {
            storage_deposit: None,
            msg: Some(alice.id().to_string()),
            memo: None,
            min_gas: None,
        },
    );
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

    let wnear_defuse_balance = env.wnear.ft_balance_of(env.defuse.id()).await?;
    assert_eq!(wnear_defuse_balance, STAKE_AMOUNT);

    assert_eq!(
        alice.near_balance().await?.total.as_millinear() + 1,
        INITIAL_BALANCE.saturating_sub(STAKE_AMOUNT).as_millinear()
    );
    let intents_balance = env.defuse.mt_balance_of(alice.id(), env.wnear.id()).await?;
    assert_eq!(intents_balance, STAKE_AMOUNT);
    assert_eq!(
        alice.near_balance().await?.total.as_millinear() + 1,
        INITIAL_BALANCE.saturating_sub(STAKE_AMOUNT).as_millinear()
    );

    Ok(())
}

#[tokio::test]
async fn test_unstake_by_withdrawing_lst_from_intents_without_storage_deposit() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let near_balance = alice.near_balance().await?;

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

    let unstake_message = unstake_message(
        alice.id(),
        WithdrawTokens::Wnear {
            storage_deposit: Some(FT_STORAGE_DEPOSIT),
            msg: None,
            memo: None,
            min_gas: None,
        },
    );
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

    let wnear_defuse_balance = env.wnear.ft_balance_of(alice.id()).await?;

    assert_eq!(
        wnear_defuse_balance,
        STAKE_AMOUNT.saturating_sub(FT_STORAGE_DEPOSIT)
    );

    env.wnear
        .near_withdraw(alice, STAKE_AMOUNT.saturating_sub(FT_STORAGE_DEPOSIT))
        .await?;

    assert_eq!(
        near_balance.total,
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(FT_STORAGE_DEPOSIT)
    );

    Ok(())
}

#[tokio::test]
async fn test_unstake_by_sending_lst_from_wnear() -> TestResult {
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

    let unstake_message = unstake_message(
        env.defuse.id(),
        WithdrawTokens::Wnear {
            storage_deposit: None,
            msg: Some(alice.id().to_string()),
            memo: None,
            min_gas: None,
        },
    );
    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &unstake_message)
        .await?;

    let total_supply = env.lst.ft_total_supply().await?;
    assert_eq!(total_supply, ZERO_AMOUNT);

    env.wait_unstake_cooldown().await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked.as_near(), INIT_LOCK.as_near());

    env.lst.withdraw(alice, &unstake_message).await?;

    let wnear_defuse_balance = env.wnear.ft_balance_of(env.defuse.id()).await?;
    assert_eq!(wnear_defuse_balance, STAKE_AMOUNT);

    assert_eq!(
        alice.near_balance().await?.total.as_millinear() + 1,
        INITIAL_BALANCE.saturating_sub(STAKE_AMOUNT).as_millinear()
    );
    let intents_balance = env.defuse.mt_balance_of(alice.id(), env.wnear.id()).await?;
    assert_eq!(intents_balance, STAKE_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_two_unstakes_by_sending_lst_from_wnear() -> TestResult {
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

    let unstake_message = unstake_message(
        env.defuse.id(),
        WithdrawTokens::Wnear {
            storage_deposit: None,
            msg: Some(alice.id().to_string()),
            memo: None,
            min_gas: None,
        },
    );
    env.lst
        .ft_transfer_call(alice, env.lst.id(), half_stake_amount, &unstake_message)
        .await?;
    env.lst
        .ft_transfer_call(alice, env.lst.id(), half_stake_amount, &unstake_message)
        .await?;

    let total_supply = env.lst.ft_total_supply().await?;
    assert_eq!(total_supply, ZERO_AMOUNT);

    env.wait_unstake_cooldown().await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked.as_near(), INIT_LOCK.as_near());

    env.lst.withdraw(alice, &unstake_message).await?;

    let wnear_defuse_balance = env.wnear.ft_balance_of(env.defuse.id()).await?;
    assert_eq!(wnear_defuse_balance, STAKE_AMOUNT);

    assert_eq!(
        alice.near_balance().await?.total.as_millinear() + 1,
        INITIAL_BALANCE.saturating_sub(STAKE_AMOUNT).as_millinear()
    );
    let intents_balance = env.defuse.mt_balance_of(alice.id(), env.wnear.id()).await?;
    assert_eq!(intents_balance, STAKE_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_stake_native_near_by_itself_and_unstake_wnear_to_itself() -> TestResult {
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

    let unstake_message = unstake_message(
        env.lst.id(),
        WithdrawTokens::Wnear {
            storage_deposit: None,
            msg: None,
            memo: None,
            min_gas: None,
        },
    );

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

    let result = env
        .lst
        .withdraw(&env.lst.as_account(), &unstake_message)
        .await?;
    dbg!(result);

    assert_eq!(
        env.lst.near_balance().await?.total,
        lst_init_balance.total.saturating_sub(STAKE_AMOUNT)
    );

    let wnear_balance = env.wnear.ft_balance_of(env.lst.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_stake_native_near_by_itself_and_unstake_wnear_to_alice() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
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

    let unstake_message = unstake_message(
        alice.id(),
        WithdrawTokens::Wnear {
            storage_deposit: None,
            msg: None,
            memo: None,
            min_gas: None,
        },
    );

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

    let result = env
        .lst
        .withdraw(&env.lst.as_account(), &unstake_message)
        .await?;
    dbg!(result);

    assert_eq!(
        env.lst.near_balance().await?.total,
        lst_init_balance
            .total
            .saturating_sub(STAKE_AMOUNT)
            .saturating_sub(ONE_YOCTO) // ft_transfer
    );

    let wnear_balance = env.wnear.ft_balance_of(alice.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    Ok(())
}
