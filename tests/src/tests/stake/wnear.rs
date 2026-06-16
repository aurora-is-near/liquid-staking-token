use liquid_staking_token::pool::WithdrawTokens;
use near_api::{NearToken, PublicKey};
use near_sdk::AccountId;
use std::str::FromStr;
use testresult::TestResult;

use crate::env::ft::{FT_STORAGE_DEPOSIT, FungibleToken};
use crate::env::mt::MultiToken;
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::env::wnear::WNear;
use crate::env::{Env, INIT_BALANCE, INIT_LOCK};
use crate::tests::assertions::assert_transaction_failure_contains;
use crate::tests::stake::HALF_OF_STAKE;
use crate::tests::{
    ONE_YOCTO, STAKE_AMOUNT, ZERO_AMOUNT, stake_message, stake_message_with_refund, unstake_message,
};

const PARTIAL_REFUND_AMOUNT: NearToken = NearToken::from_near(250);
const INVALID_TOKEN_ACCOUNT_ERROR: &str = "Invalid token account ID";

fn partial_refund_message(refund_amount: NearToken) -> String {
    refund_amount.as_yoctonear().to_string()
}

#[tokio::test]
async fn test_stake_with_wnear_and_get_on_intents() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    env.wnear.near_deposit(alice, STAKE_AMOUNT).await?;

    let wnear_balance = env.wnear.ft_balance_of(alice.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    let message = stake_message(env.intents.id(), false, Some(alice.id()));
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

    let message = stake_message(alice.id(), false, None::<&AccountId>);
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

    let message = stake_message(env.intents.id(), false, Some(bob.id()));
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

    let message = stake_message(bob.id(), false, None::<&AccountId>);
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
async fn test_stake_with_wnear_and_get_on_nep141_to_unregistered() -> TestResult {
    let env = Env::builder().build().await?;

    let alice = env.alice();
    let unregistered: AccountId = "unregistered.sandbox".parse()?;
    let alice_native_balance_before = alice.near_balance().await?;

    env.wnear.near_deposit(alice, STAKE_AMOUNT).await?;

    let wnear_balance = env.wnear.ft_balance_of(alice.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    let message = stake_message(&unregistered, false, None::<&str>);

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

// We assume that extra storage deposits are not refunded to the sender and left on the contract
// account id as rewards.
#[tokio::test]
async fn test_stake_with_wnear_with_registration_for_registered() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?.total;
    let lst_native_balance_before = env.lst.near_balance().await?.total;

    env.wnear
        .near_deposit(alice, STAKE_AMOUNT.saturating_add(FT_STORAGE_DEPOSIT))
        .await?;

    env.wnear
        .ft_transfer_call(
            alice,
            env.lst.id(),
            STAKE_AMOUNT.saturating_add(FT_STORAGE_DEPOSIT),
            stake_message(alice.id(), true, None::<&str>),
        )
        .await?;

    let lst_balance = env.lst.near_balance().await?;
    assert_eq!(lst_balance.locked, INIT_LOCK.saturating_add(STAKE_AMOUNT));

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );

    assert_eq!(
        env.lst.near_balance().await?.total,
        lst_native_balance_before.saturating_add(FT_STORAGE_DEPOSIT)
    );
    assert_eq!(
        alice.near_balance().await?.total,
        alice_native_balance_before
            .saturating_sub(STAKE_AMOUNT)
            .saturating_sub(FT_STORAGE_DEPOSIT)
            .saturating_sub(ONE_YOCTO)
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_wnear_and_get_on_intents_to_unregistered() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;

    let alice = env.alice();
    let alice_native_balance_before = alice.near_balance().await?;

    env.wnear
        .near_deposit(alice, STAKE_AMOUNT.saturating_add(FT_STORAGE_DEPOSIT))
        .await?;

    let wnear_balance = env.wnear.ft_balance_of(alice.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    let message = stake_message(env.intents.id(), false, Some(alice.id()));

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
            is_storage_deposit: false,
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
                false,
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
        INIT_BALANCE.saturating_add(STAKE_AMOUNT) // The staked amount is still on the contract balance.
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
async fn test_stake_with_wnear_and_ft_on_transfer_panic_with_native_refund() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let ft_receiver = env.deploy_ft_receiver().await?;
    let alice_native_balance_before = alice.near_balance().await?;

    env.wnear.near_deposit(alice, STAKE_AMOUNT).await?;

    let wnear_balance = env.wnear.ft_balance_of(alice.id()).await?;
    assert_eq!(wnear_balance, STAKE_AMOUNT);

    let refund_message = unstake_message(alice.id(), &WithdrawTokens::Native);

    env.wnear
        .ft_transfer_call(
            alice,
            env.lst.id(),
            STAKE_AMOUNT,
            stake_message_with_refund(
                ft_receiver.id(),
                false,
                Some("invalid refund amount"),
                Some(&refund_message),
            ),
        )
        .await?;

    assert_eq!(env.lst.ft_balance_of(ft_receiver.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, STAKE_AMOUNT);

    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE.saturating_add(STAKE_AMOUNT)
    );

    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
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

    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);
    assert_eq!(env.lst.near_balance().await?.locked, INIT_LOCK);
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        alice.near_balance().await?.total,
        alice_native_balance_before.total.saturating_sub(ONE_YOCTO)
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_wnear_and_partial_nep141_refund_with_refund_message() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let ft_receiver = env.deploy_ft_receiver().await?;
    let alice_native_balance_before = alice.near_balance().await?;
    let consumed_amount = STAKE_AMOUNT.saturating_sub(PARTIAL_REFUND_AMOUNT);
    let refund_message = unstake_message(
        alice.id(),
        &WithdrawTokens::Wnear {
            is_storage_deposit: false,
            msg: None,
            memo: None,
            min_gas: None,
        },
    );

    env.wnear.near_deposit(alice, STAKE_AMOUNT).await?;

    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);

    env.wnear
        .ft_transfer_call(
            alice,
            env.lst.id(),
            STAKE_AMOUNT,
            stake_message_with_refund(
                ft_receiver.id(),
                false,
                Some(partial_refund_message(PARTIAL_REFUND_AMOUNT)),
                Some(&refund_message),
            ),
        )
        .await?;

    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE.saturating_add(STAKE_AMOUNT)
    );

    assert_eq!(
        env.lst.ft_balance_of(ft_receiver.id()).await?,
        consumed_amount
    );
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(consumed_amount)
    );
    assert_eq!(
        env.lst.get_total_pending_withdrawals().await?,
        PARTIAL_REFUND_AMOUNT
    );
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
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

    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);
    assert_eq!(
        env.lst.ft_balance_of(ft_receiver.id()).await?,
        consumed_amount
    );
    assert_eq!(
        env.wnear.ft_balance_of(alice.id()).await?,
        PARTIAL_REFUND_AMOUNT
    );
    assert_eq!(
        alice_native_balance_before.total,
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(STAKE_AMOUNT) // The partial refund is returned as wNEAR
            .saturating_add(ONE_YOCTO) // ft_transfer_call deposits 1 yoctoNEAR to the contract
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_with_non_wnear_non_lst_token_fails() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let other_ft = env.deploy_ft_token().await?;

    let result = env
        .lst
        .ft_on_transfer(
            &other_ft.as_account(),
            alice.id(),
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&AccountId>),
        )
        .await;

    assert_transaction_failure_contains(result, INVALID_TOKEN_ACCOUNT_ERROR);

    assert_eq!(env.lst.near_balance().await?.locked, INIT_LOCK);
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, ZERO_AMOUNT);

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
            is_storage_deposit: false,
            msg: None,
            memo: None,
            min_gas: None,
        },
    );

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message_with_refund(alice.id(), false, Some(bob.id()), Some(&refund_message)),
        )
        .await?;

    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        alice.near_balance().await?.total,
        INIT_BALANCE
            .saturating_sub(STAKE_AMOUNT)
            .saturating_sub(ONE_YOCTO)
    );
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &refund_message).await?;

    assert_eq!(env.lst.near_balance().await?.locked, INIT_LOCK);

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(
        alice.near_balance().await?.total,
        INIT_BALANCE
            .saturating_sub(STAKE_AMOUNT)
            .saturating_sub(ONE_YOCTO)
    );

    assert_eq!(env.lst.ft_balance_of(bob.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        bob.near_balance().await?.total,
        INIT_BALANCE.saturating_sub(ONE_YOCTO)
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_wnear_and_sending_lst_tokens_to_contract_with_refund() -> TestResult {
    let env = Env::builder().build().await?;
    let lst_receiver = env.deploy_ft_receiver().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let refund_message = unstake_message(alice.id(), &WithdrawTokens::Native);

    env.wnear
        .ft_storage_deposit(&env.wnear.as_account(), lst_receiver.id())
        .await?;
    env.wnear.near_deposit(alice, STAKE_AMOUNT).await?;

    env.wnear
        .ft_transfer_call(
            alice,
            env.lst.id(),
            STAKE_AMOUNT,
            stake_message_with_refund(
                lst_receiver.id(),
                false,
                Some(HALF_OF_STAKE),
                Some(&refund_message),
            ),
        )
        .await?;

    assert_eq!(
        env.lst.ft_balance_of(lst_receiver.id()).await?,
        HALF_OF_STAKE
    );
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(HALF_OF_STAKE)
    );
    assert_eq!(
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(STAKE_AMOUNT)
            .saturating_add(ONE_YOCTO),
        alice_balance_before.total
    );

    env.wait_unstake_cooldown().await?;
    env.lst.withdraw(alice, &refund_message).await?;

    assert_eq!(
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(HALF_OF_STAKE)
            .saturating_add(ONE_YOCTO),
        alice_balance_before.total
    );

    Ok(())
}

#[tokio::test]
#[ignore = "Requires panic in the unstake flow; cannot be triggered in e2e"]
async fn test_stake_wnear_and_sending_lst_tokens_to_contract_with_refund_and_unstake_panic()
-> TestResult {
    let env = Env::builder().build().await?;
    let lst_receiver = env.deploy_ft_receiver().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let refund_message = unstake_message(
        alice.id(),
        &WithdrawTokens::Wnear {
            is_storage_deposit: false,
            msg: None,
            memo: None,
            min_gas: None,
        },
    );

    env.wnear
        .near_deposit(alice, STAKE_AMOUNT.saturating_mul(2))
        .await?;
    env.wnear
        .ft_transfer(alice, env.lst.id(), STAKE_AMOUNT)
        .await?;

    env.wnear
        .ft_transfer_call(
            alice,
            env.lst.id(),
            STAKE_AMOUNT,
            stake_message_with_refund(
                lst_receiver.id(),
                false,
                Some(HALF_OF_STAKE),
                Some(&refund_message),
            ),
        )
        .await?;

    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        env.lst.ft_balance_of(lst_receiver.id()).await?,
        HALF_OF_STAKE
    );
    assert_eq!(
        env.lst.ft_balance_of(env.lst.id()).await?,
        INIT_LOCK.saturating_add(HALF_OF_STAKE) // Refunded tokens are stuck on the contract because of the panic in the unstake flow.
    );

    env.wait_unstake_cooldown().await?;

    let result = env.lst.withdraw(alice, &refund_message).await;

    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Account is not found in the unstake queue")
    );

    assert_eq!(
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(STAKE_AMOUNT.saturating_mul(2))
            .saturating_add(ONE_YOCTO) // ft_transfer
            .saturating_add(ONE_YOCTO), // ft_transfer_call
        alice_balance_before.total
    );

    assert_eq!(env.wnear.ft_balance_of(env.lst.id()).await?, STAKE_AMOUNT);
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        env.wnear.ft_balance_of(lst_receiver.id()).await?,
        ZERO_AMOUNT
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_wnear_and_sending_lst_tokens_to_lst_contract() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let double_stake_amount = STAKE_AMOUNT.saturating_mul(2);

    env.wnear.near_deposit(alice, double_stake_amount).await?;
    env.wnear
        .ft_transfer(alice, env.lst.id(), STAKE_AMOUNT)
        .await?;

    env.wnear
        .ft_transfer_call(
            alice,
            env.lst.id(),
            STAKE_AMOUNT,
            stake_message(env.lst.id(), false, None::<&str>),
        )
        .await?;

    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        env.lst.ft_balance_of(env.lst.id()).await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        alice.near_balance().await?.total,
        alice_balance_before
            .total
            .saturating_sub(double_stake_amount)
            .saturating_sub(ONE_YOCTO)
            .saturating_sub(ONE_YOCTO)
    );
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.wnear.ft_balance_of(env.lst.id()).await?, STAKE_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_stake_wnear_and_sending_lst_tokens_to_lst_contract_with_random_msg() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let double_stake_amount = STAKE_AMOUNT.saturating_mul(2);

    env.wnear.near_deposit(alice, double_stake_amount).await?;
    env.wnear
        .ft_transfer(alice, env.lst.id(), STAKE_AMOUNT)
        .await?;

    env.wnear
        .ft_transfer_call(
            alice,
            env.lst.id(),
            STAKE_AMOUNT,
            stake_message(env.lst.id(), false, Some("random message")),
        )
        .await?;

    // `ft_on_transfer` panics on the contract, since the message isn't recognizable.
    // No refund message, so the tokens should be left on the receiver contract (LST).

    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        env.lst.ft_balance_of(env.lst.id()).await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        alice.near_balance().await?.total,
        alice_balance_before
            .total
            .saturating_sub(double_stake_amount)
            .saturating_sub(ONE_YOCTO)
            .saturating_sub(ONE_YOCTO)
    );
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.wnear.ft_balance_of(env.lst.id()).await?, STAKE_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_stake_wnear_and_sending_lst_tokens_to_lst_contract_with_random_msg_and_refund_back()
-> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let refund_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    let double_stake_amount = STAKE_AMOUNT.saturating_mul(2);

    env.wnear.near_deposit(alice, double_stake_amount).await?;
    env.wnear
        .ft_transfer(alice, env.lst.id(), STAKE_AMOUNT)
        .await?;

    env.wnear
        .ft_transfer_call(
            alice,
            env.lst.id(),
            STAKE_AMOUNT,
            stake_message_with_refund(
                env.lst.id(),
                false,
                Some("random message"),
                Some(&refund_message),
            ),
        )
        .await?;

    // `ft_on_transfer` panics on the contract, since the message isn't recognizable.

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(env.lst.ft_balance_of(env.lst.id()).await?, INIT_LOCK);
    assert_eq!(
        alice.near_balance().await?.total,
        alice_balance_before
            .total
            .saturating_sub(double_stake_amount)
            .saturating_sub(ONE_YOCTO)
            .saturating_sub(ONE_YOCTO)
    );

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &refund_message).await?;

    assert_eq!(
        alice.near_balance().await?.total,
        alice_balance_before
            .total
            .saturating_sub(STAKE_AMOUNT)
            .saturating_sub(ONE_YOCTO)
            .saturating_sub(ONE_YOCTO)
    );
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.wnear.ft_balance_of(env.lst.id()).await?, STAKE_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_stake_wnear_and_sending_lst_tokens_to_lst_contract_with_random_msg_and_refund_to_lst()
-> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let double_stake_amount = STAKE_AMOUNT.saturating_mul(2);
    let refund_message = unstake_message(
        env.lst.id(),
        &WithdrawTokens::Wnear {
            is_storage_deposit: false,
            msg: None,
            memo: None,
            min_gas: None,
        },
    );
    let stake_message = stake_message_with_refund(
        env.lst.id(),
        false,
        Some("random message"),
        Some(&refund_message),
    );

    env.wnear.near_deposit(alice, double_stake_amount).await?;
    env.wnear
        .ft_transfer(alice, env.lst.id(), STAKE_AMOUNT)
        .await?;

    env.wnear
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, stake_message)
        .await?;

    // `ft_on_transfer` panics on the contract, since the message isn't recognizable.

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(env.lst.ft_balance_of(env.lst.id()).await?, INIT_LOCK);
    assert_eq!(
        alice.near_balance().await?.total,
        alice_balance_before
            .total
            .saturating_sub(double_stake_amount)
            .saturating_sub(ONE_YOCTO)
            .saturating_sub(ONE_YOCTO)
    );

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &refund_message).await?;

    assert_eq!(
        alice.near_balance().await?.total,
        alice_balance_before
            .total
            .saturating_sub(double_stake_amount)
            .saturating_sub(ONE_YOCTO)
            .saturating_sub(ONE_YOCTO)
    );
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        env.wnear.ft_balance_of(env.lst.id()).await?,
        double_stake_amount
    );

    Ok(())
}

#[tokio::test]
#[ignore = "Requires panic in the unstake flow; cannot be triggered in e2e"]
async fn test_stake_wnear_and_sending_lst_tokens_to_lst_contract_with_random_msg_and_refund_to_lst_and_panic_in_unstake()
-> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let refund_message = unstake_message(
        env.lst.id(),
        &WithdrawTokens::Wnear {
            is_storage_deposit: false,
            msg: None,
            memo: None,
            min_gas: None,
        },
    );
    let stake_message = stake_message_with_refund(
        env.lst.id(),
        false,
        Some("random message"),
        Some(&refund_message),
    );

    env.lst.stake(alice, STAKE_AMOUNT, stake_message).await?;

    // `ft_on_transfer` panics on the contract, since the message isn't recognizable.

    assert_eq!(
        env.lst.ft_total_supply().await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        env.lst.ft_balance_of(env.lst.id()).await?,
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        alice.near_balance().await?.total,
        alice_balance_before.total.saturating_sub(STAKE_AMOUNT)
    );
    assert_eq!(env.wnear.ft_balance_of(env.lst.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE.saturating_add(STAKE_AMOUNT)
    );

    Ok(())
}

#[ignore = "After making a stake with a wrong public key, a validator stops being a validator and stops producing blocks"]
#[tokio::test]
async fn test_stake_wnear_with_using_wrong_validator_public_key() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let lst_balance_before = env.lst.near_balance().await?;

    env.lst
        .set_validator_public_key(
            PublicKey::from_str("ed25519:UmmZ5Zq5t1i2JFA4aG5T8uhPn6BwMbnDNcEnfJAHjmL").unwrap(),
        )
        .await?;

    env.wnear.near_deposit(alice, STAKE_AMOUNT).await?;
    env.wnear
        .ft_transfer_call(
            alice,
            env.lst.id(),
            STAKE_AMOUNT,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(env.lst.near_balance().await?, lst_balance_before);
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(
        alice.near_balance().await?.total,
        alice_balance_before
            .total
            .saturating_sub(STAKE_AMOUNT)
            .saturating_sub(ONE_YOCTO)
    );

    Ok(())
}

#[tokio::test]
async fn test_stake_wnear_with_deposit_less_than_needed_for_registration() -> TestResult {
    let env = Env::builder().without_storage_deposit().build().await?;
    let alice = env.alice();
    let alice_balance_before = alice.near_balance().await?;
    let lst_balance_before = env.lst.near_balance().await?;
    let deposit = FT_STORAGE_DEPOSIT.saturating_sub(ONE_YOCTO);

    env.wnear
        .ft_storage_deposit(&env.wnear.as_account(), alice.id())
        .await?;
    env.wnear.near_deposit(alice, STAKE_AMOUNT).await?;

    env.wnear
        .ft_transfer_call(
            alice,
            env.lst.id(),
            deposit,
            stake_message(alice.id(), true, None::<&str>),
        )
        .await?;

    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    assert_eq!(env.lst.near_balance().await?, lst_balance_before);
    assert_eq!(env.wnear.ft_balance_of(env.lst.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(
        alice.near_balance().await?.total,
        alice_balance_before
            .total
            .saturating_sub(STAKE_AMOUNT)
            .saturating_sub(ONE_YOCTO)
    );

    Ok(())
}

#[tokio::test]
async fn test_wnear_stake_with_exceeded_amount_of_gas() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let receiver = env.deploy_ft_receiver().await?;

    env.wnear.near_deposit(alice, STAKE_AMOUNT).await?;

    let refund_message = unstake_message(alice.id(), &WithdrawTokens::Native);

    let message = near_sdk::serde_json::json!({
        "receiver_id": receiver.id(),
        "is_storage_deposit": false,
        "msg": "0",
        "min_gas": 250_000_000_000_000u64, // too much gas
        "refund_message": refund_message,
    });

    env.wnear
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, message)
        .await?;

    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.wnear.ft_balance_of(env.lst.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.lst.ft_balance_of(receiver.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        env.lst.ft_balance_of(env.lst.id()).await?, // LST tokens stay on the contract because of exceeded gas
        INIT_LOCK.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(
        env.lst.ft_total_supply().await?,
        env.lst.ft_balance_of(env.lst.id()).await?
    );

    Ok(())
}
