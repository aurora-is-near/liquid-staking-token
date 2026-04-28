use liquid_staking_token::pool::WithdrawTokens;
use near_sdk::AccountId;
use testresult::TestResult;

use crate::env::ft::{FT_STORAGE_DEPOSIT, FungibleToken};
use crate::env::mt::MultiToken;
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::env::wnear::WNear;
use crate::env::{Env, INIT_LOCK, INITIAL_BALANCE};
use crate::tests::{
    ONE_YOCTO, STAKE_AMOUNT, ZERO_AMOUNT, stake_message, stake_message_with_refund, unstake_message,
};

#[tokio::test]
async fn test_stake_with_wnear_and_get_on_intents() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    env.wnear.near_deposit(alice, STAKE_AMOUNT).await?;

    let wnear_balance = env.wnear.ft_balance_of(alice.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    let message = stake_message(env.intents.id(), None, Some(alice.id()));
    env.wnear
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, message)
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let intents_lst_balance = env.lst.ft_balance_of(env.intents.id()).await?;
    assert_eq!(intents_lst_balance, STAKE_AMOUNT);

    let intents_lst_balance = env.intents.mt_balance_of(alice.id(), env.lst.id()).await?;
    assert_eq!(intents_lst_balance, STAKE_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice_native_balance_after.total)
            .saturating_sub(ONE_YOCTO), // ft_transfer_call deposits 1 yoctoNEAR to the contract
        STAKE_AMOUNT
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_wnear_and_get_on_nep141() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    env.wnear.near_deposit(alice, STAKE_AMOUNT).await?;

    let wnear_balance = env.wnear.ft_balance_of(alice.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    let message = stake_message(alice.id(), None, None::<&AccountId>);
    env.wnear
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, message)
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let intents_lst_balance = env.lst.ft_balance_of(env.intents.id()).await?;
    assert_eq!(intents_lst_balance, ZERO_AMOUNT);
    let alice_lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(alice_lst_balance, STAKE_AMOUNT);

    let intents_lst_balance = env.intents.mt_balance_of(alice.id(), env.lst.id()).await?;
    assert_eq!(intents_lst_balance, ZERO_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice_native_balance_after.total)
            .saturating_sub(ONE_YOCTO), // ft_transfer_call deposits 1 yoctoNEAR to the contract
        STAKE_AMOUNT
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_wrapped_near_and_get_on_intents_to_bob() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();
    let alice_native_balance_before = alice.near_balance().await?;

    env.wnear.near_deposit(alice, STAKE_AMOUNT).await?;

    let wnear_balance = env.wnear.ft_balance_of(alice.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    let message = stake_message(env.intents.id(), None, Some(bob.id()));
    env.wnear
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, message)
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let intents_lst_balance = env.lst.ft_balance_of(env.intents.id()).await?;
    assert_eq!(intents_lst_balance, STAKE_AMOUNT);

    let intents_lst_balance = env.intents.mt_balance_of(alice.id(), env.lst.id()).await?;
    assert_eq!(intents_lst_balance, ZERO_AMOUNT);
    let intents_lst_balance = env.intents.mt_balance_of(bob.id(), env.lst.id()).await?;
    assert_eq!(intents_lst_balance, STAKE_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice_native_balance_after.total)
            .saturating_sub(ONE_YOCTO), // ft_transfer_call deposits 1 yoctoNEAR to the contract
        STAKE_AMOUNT
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_wnear_and_get_on_nep141_to_bob() -> TestResult {
    let env = Env::builder().build().await?;

    let alice = env.alice();
    let bob = env.bob();
    let alice_native_balance_before = alice.near_balance().await?;

    env.wnear.near_deposit(alice, STAKE_AMOUNT).await?;

    let wnear_balance = env.wnear.ft_balance_of(alice.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    let message = stake_message(bob.id(), None, None::<&AccountId>);
    env.wnear
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, message)
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let intents_lst_balance = env.lst.ft_balance_of(env.intents.id()).await?;
    assert_eq!(intents_lst_balance, ZERO_AMOUNT);
    let alice_lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(alice_lst_balance, ZERO_AMOUNT);
    let bob_lst_balance = env.lst.ft_balance_of(bob.id()).await?;
    assert_eq!(bob_lst_balance, STAKE_AMOUNT);

    let intents_lst_balance = env.intents.mt_balance_of(alice.id(), env.lst.id()).await?;
    assert_eq!(intents_lst_balance, ZERO_AMOUNT);
    let intents_lst_balance = env.intents.mt_balance_of(bob.id(), env.lst.id()).await?;
    assert_eq!(intents_lst_balance, ZERO_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice_native_balance_after.total)
            .saturating_sub(ONE_YOCTO), // ft_transfer_call deposits 1 yoctoNEAR to the contract
        STAKE_AMOUNT
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_wrapped_near_and_get_on_nep141_to_unregistered() -> TestResult {
    let env = Env::builder().build().await?;

    let alice = env.alice();
    let unregistered: AccountId = "unregistered.sandbox".parse()?;
    let alice_native_balance_before = alice.near_balance().await?;

    env.wnear.near_deposit(alice, STAKE_AMOUNT).await?;

    let wnear_balance = env.wnear.ft_balance_of(alice.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    let message = stake_message(&unregistered, None, None::<&AccountId>);

    env.wnear
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, message)
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK);

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    let wnear_balance = env.wnear.ft_balance_of(alice.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    let intents_lst_balance = env.lst.ft_balance_of(env.intents.id()).await?;
    assert_eq!(intents_lst_balance, ZERO_AMOUNT);
    let alice_lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(alice_lst_balance, ZERO_AMOUNT);
    let bob_lst_balance = env.lst.ft_balance_of(&unregistered).await?;
    assert_eq!(bob_lst_balance, ZERO_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice_native_balance_after.total)
            .saturating_sub(ONE_YOCTO), // ft_transfer_call deposits 1 yoctoNEAR to the contract
        STAKE_AMOUNT
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_wrapped_near_and_get_on_intents_to_unregistered() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;

    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    env.wnear
        .near_deposit(alice, STAKE_AMOUNT.saturating_add(FT_STORAGE_DEPOSIT))
        .await?;

    let wnear_balance = env.wnear.ft_balance_of(alice.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    let message = stake_message(env.intents.id(), None, Some(alice.id()));

    env.wnear
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, message)
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    let intents_lst_balance = env.intents.mt_balance_of(alice.id(), env.lst.id()).await?;
    assert_eq!(intents_lst_balance, STAKE_AMOUNT);

    let intents_lst_balance = env.lst.ft_balance_of(env.intents.id()).await?;
    assert_eq!(intents_lst_balance, STAKE_AMOUNT);
    let alice_lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(alice_lst_balance, ZERO_AMOUNT);
    let bob_lst_balance = env.lst.ft_balance_of(alice.id()).await?;
    assert_eq!(bob_lst_balance, ZERO_AMOUNT);

    let alice_native_balance_after = alice.near_balance().await?;
    assert_eq!(
        alice_native_balance_before
            .total
            .saturating_sub(alice_native_balance_after.total)
            .saturating_sub(FT_STORAGE_DEPOSIT) // storage_deposit in wNEAR
            .saturating_sub(ONE_YOCTO), // ft_transfer_call deposits 1 yoctoNEAR to the contract
        STAKE_AMOUNT
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_wnear_and_get_on_intents_with_wrong_message() -> TestResult {
    let env = Env::builder().build().await?;

    let alice = env.alice();

    env.wnear.near_deposit(alice, STAKE_AMOUNT).await?;

    let wnear_balance = env.wnear.ft_balance_of(alice.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    env.wnear
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, "wrong message")
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK);

    let intents_lst_balance = env.lst.ft_balance_of(env.intents.id()).await?;
    assert_eq!(intents_lst_balance, ZERO_AMOUNT);

    let wnear_balance = env.wnear.ft_balance_of(alice.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_stake_with_wnear_and_to_send_on_intents_with_bad_account_with_wnear_refund()
-> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    env.wnear.near_deposit(alice, STAKE_AMOUNT).await?;

    let refund_message = unstake_message(
        alice.id(),
        &WithdrawTokens::Wnear {
            storage_deposit: None,
            msg: None,
            memo: None,
            min_gas: None,
        },
    );

    env.wnear
        .ft_transfer_call(
            alice,
            env.lst.id(),
            STAKE_AMOUNT,
            stake_message_with_refund(
                env.intents.id(),
                None,
                Some("a2933a$$%$1!@!#@!@"),
                Some(&refund_message),
            ),
        )
        .await?;

    assert_eq!(env.lst.ft_balance_of(env.intents.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    assert_eq!(env.lst.get_total_staked_balance().await?, INIT_LOCK);
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, STAKE_AMOUNT);
    assert_eq!(
        env.lst.get_total_balance().await?,
        INITIAL_BALANCE.saturating_add(STAKE_AMOUNT) // The staked amount is still on the contract balance.
    );

    assert_eq!(
        env.intents.mt_balance_of(alice.id(), env.lst.id()).await?,
        ZERO_AMOUNT
    ); // No tokens on intents minted
    assert_eq!(
        alice_native_balance_before.total,
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(STAKE_AMOUNT) // Alice's balance was decreased by STAKE_AMOUNT
            .saturating_add(ONE_YOCTO) // ft_transfer_call deposits 1 yoctoNEAR to the contract
    );

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &refund_message).await?;

    let wnear_alice_balance = env.wnear.ft_balance_of(alice.id()).await?;
    assert_eq!(wnear_alice_balance, STAKE_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_stake_with_attempt_to_get_shared_tokens_on_contract() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();
    let refund_message = unstake_message(
        alice.id(),
        &WithdrawTokens::Wnear {
            storage_deposit: None,
            msg: None,
            memo: None,
            min_gas: None,
        },
    );

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message_with_refund(alice.id(), None, Some(bob.id()), Some(&refund_message)),
        )
        .await?;

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(
        env.lst.near_balance().await?.locked,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );

    assert_eq!(
        alice.near_balance().await?.total,
        INITIAL_BALANCE
            .saturating_sub(STAKE_AMOUNT)
            .saturating_sub(ONE_YOCTO)
    );

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &refund_message).await?;

    assert_eq!(env.lst.near_balance().await?.locked, INIT_LOCK);

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(
        alice.near_balance().await?.total,
        INITIAL_BALANCE
            .saturating_sub(STAKE_AMOUNT)
            .saturating_sub(ONE_YOCTO)
    );

    assert_eq!(env.lst.ft_balance_of(bob.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        bob.near_balance().await?.total,
        INITIAL_BALANCE.saturating_sub(ONE_YOCTO)
    );

    Ok(())
}
