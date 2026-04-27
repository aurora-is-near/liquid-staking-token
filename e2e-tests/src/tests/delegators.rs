use liquid_staking_token::pool::WithdrawTokens;
use testresult::TestResult;

use crate::env::Env;
use crate::env::ft::FungibleToken;
use crate::env::pool::StakingPool;
use crate::tests::{STAKE_AMOUNT, stake_message, stake_message_with_refund, unstake_message};

#[tokio::test]
async fn test_init_with_init_lock_counts_owner_as_delegator() -> TestResult {
    let env = Env::builder().build().await?;
    // Owner gets LST from init_lock, so starts as 1 delegator
    assert_eq!(env.lst.get_number_of_accounts().await?, 1);
    Ok(())
}

#[tokio::test]
async fn test_stake_new_account_increments_delegators() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    assert_eq!(env.lst.get_number_of_accounts().await?, 1);

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;

    assert_eq!(env.lst.get_number_of_accounts().await?, 2);
    Ok(())
}

#[tokio::test]
async fn test_stake_twice_same_account_no_double_count() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;
    assert_eq!(env.lst.get_number_of_accounts().await?, 2);

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;
    assert_eq!(env.lst.get_number_of_accounts().await?, 2);

    Ok(())
}

#[tokio::test]
async fn test_two_users_stake_counts_both() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;
    assert_eq!(env.lst.get_number_of_accounts().await?, 2);

    env.lst
        .stake(
            bob,
            STAKE_AMOUNT,
            stake_message(bob.id(), None, None::<&str>),
        )
        .await?;
    assert_eq!(env.lst.get_number_of_accounts().await?, 3);

    Ok(())
}

#[tokio::test]
async fn test_unstake_all_decrements_delegators() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;
    assert_eq!(env.lst.get_number_of_accounts().await?, 2);

    let unstake_msg = unstake_message(alice.id(), &WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &unstake_msg)
        .await?;

    assert_eq!(env.lst.get_number_of_accounts().await?, 1);
    Ok(())
}

#[tokio::test]
async fn test_partial_unstake_keeps_delegator() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;
    assert_eq!(env.lst.get_number_of_accounts().await?, 2);

    let unstake_msg = unstake_message(alice.id(), &WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(
            alice,
            env.lst.id(),
            STAKE_AMOUNT.saturating_div(2),
            &unstake_msg,
        )
        .await?;

    assert_eq!(env.lst.get_number_of_accounts().await?, 2);
    Ok(())
}

#[tokio::test]
async fn test_ft_transfer_partial_to_new_receiver_increments() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;
    assert_eq!(env.lst.get_number_of_accounts().await?, 2);

    // Transfer half to Bob (Bob is new, Alice keeps balance)
    env.lst
        .ft_transfer(alice, bob.id(), STAKE_AMOUNT.saturating_div(2))
        .await?;

    assert_eq!(env.lst.get_number_of_accounts().await?, 3); // owner + alice + bob
    Ok(())
}

#[tokio::test]
async fn test_ft_transfer_all_to_new_receiver_net_zero() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;
    assert_eq!(env.lst.get_number_of_accounts().await?, 2);

    // Transfer ALL to Bob: +1 Bob, -1 Alice = net 0
    env.lst.ft_transfer(alice, bob.id(), STAKE_AMOUNT).await?;

    assert_eq!(env.lst.get_number_of_accounts().await?, 2); // owner + bob
    Ok(())
}

#[tokio::test]
async fn test_ft_transfer_all_to_existing_receiver_decrements() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    // Both stake
    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;
    env.lst
        .stake(
            bob,
            STAKE_AMOUNT,
            stake_message(bob.id(), None, None::<&str>),
        )
        .await?;
    assert_eq!(env.lst.get_number_of_accounts().await?, 3);

    // Alice sends all to Bob (Bob already has balance)
    env.lst.ft_transfer(alice, bob.id(), STAKE_AMOUNT).await?;

    assert_eq!(env.lst.get_number_of_accounts().await?, 2); // owner + bob
    Ok(())
}

#[tokio::test]
async fn test_ft_transfer_call_all_to_intents_no_refund() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(env.intents.id(), None, Some(alice.id())),
        )
        .await?;

    // Tokens went to intents via ft_on_transfer. intents is a new delegator.
    // The LST contract minted tokens to intents (via modify_total_staked_amount).
    assert_eq!(env.lst.get_number_of_accounts().await?, 2); // owner + intents

    Ok(())
}

#[tokio::test]
async fn test_ft_transfer_call_partial_to_intents_no_refund() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    // Stake to alice first
    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;
    assert_eq!(env.lst.get_number_of_accounts().await?, 2);

    // Now stake to intents (alice also keeps her balance)
    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(env.intents.id(), None, Some(alice.id())),
        )
        .await?;

    assert_eq!(env.lst.get_number_of_accounts().await?, 3); // owner + alice + intents
    Ok(())
}

#[tokio::test]
async fn test_ft_transfer_call_full_refund_no_change() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let refund_message = unstake_message(alice.id(), &WithdrawTokens::Native);

    // Stake with bad intents msg → triggers panic in ft_on_transfer → full refund
    // refund_message routes to unstake queue
    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message_with_refund(
                env.intents.id(),
                None,
                Some("a2933a$$%$1!@!#@!@"), // Bad msg triggers rejection
                Some(&refund_message),
            ),
        )
        .await?;

    // Tokens were refunded via unstake. intents was incremented then decremented
    // via ft_resolve_transfer. No new delegators remain from the rejected transfer.
    assert_eq!(env.lst.get_number_of_accounts().await?, 1); // just owner
    Ok(())
}

#[tokio::test]
async fn test_ft_transfer_call_full_refund_sender_keeps_delegator_status() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();

    // First, give alice some tokens
    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;
    assert_eq!(env.lst.get_number_of_accounts().await?, 2);

    // Now stake more with bad intents msg → triggers rejection → full refund via unstake
    let refund_message = unstake_message(alice.id(), &WithdrawTokens::Native);
    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message_with_refund(
                env.intents.id(),
                None,
                Some("a2933a$$%$1!@!#@!@"),
                Some(&refund_message),
            ),
        )
        .await?;

    // Alice still has her original STAKE_AMOUNT. The rejected stake was refunded.
    assert_eq!(env.lst.get_number_of_accounts().await?, 2); // owner + alice
    Ok(())
}

#[tokio::test]
async fn test_stake_transfer_all_unstake_all_returns_to_one() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    // Alice stakes
    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;
    assert_eq!(env.lst.get_number_of_accounts().await?, 2);

    // Alice transfers all to Bob
    env.lst.ft_transfer(alice, bob.id(), STAKE_AMOUNT).await?;
    assert_eq!(env.lst.get_number_of_accounts().await?, 2); // owner + bob

    // Bob unstakes all
    let unstake_msg = unstake_message(bob.id(), &WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(bob, env.lst.id(), STAKE_AMOUNT, &unstake_msg)
        .await?;

    assert_eq!(env.lst.get_number_of_accounts().await?, 1); // just owner
    Ok(())
}

#[tokio::test]
async fn test_alice_bob_stake_alice_unstakes_bob_transfers_to_alice() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    // Both stake
    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;

    env.lst
        .stake(
            bob,
            STAKE_AMOUNT,
            stake_message(bob.id(), None, None::<&str>),
        )
        .await?;
    assert_eq!(env.lst.get_number_of_accounts().await?, 3); // owner + alice + bob

    // Alice unstakes all
    let unstake_msg = unstake_message(alice.id(), &WithdrawTokens::Native);
    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &unstake_msg)
        .await?;
    assert_eq!(env.lst.get_number_of_accounts().await?, 2); // owner + bob

    // Bob transfers all to Alice (Alice becomes delegator again, Bob leaves)
    env.lst.ft_transfer(bob, alice.id(), STAKE_AMOUNT).await?;
    assert_eq!(env.lst.get_number_of_accounts().await?, 2); // owner + alice

    Ok(())
}

#[tokio::test]
async fn test_self_stake_does_not_double_count_contract() -> TestResult {
    let env = Env::builder().build().await?;

    // Contract already has init_lock balance (count = 1)
    // Self-stake: receiver = contract (already has balance) → count unchanged
    env.lst
        .stake(
            &env.lst.as_account(),
            STAKE_AMOUNT,
            stake_message(env.lst.id(), None, None::<&str>),
        )
        .await?;

    assert_eq!(env.lst.get_number_of_accounts().await?, 1); // still just the contract
    Ok(())
}
