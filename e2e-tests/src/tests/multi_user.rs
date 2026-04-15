use liquid_staking_token::pool::WithdrawTokens;
use near_api::NearToken;
use testresult::TestResult;

use crate::env::ft::FungibleToken;
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::env::{Env, INIT_LOCK, INITIAL_BALANCE};
use crate::tests::{STAKE_AMOUNT, ZERO_AMOUNT, stake_message, unstake_message};

/// Alice and Bob stake independently, unstake with different messages (different
/// receiver_ids produce different queue keys), and each withdraws their own funds
/// without interfering with the other.
#[tokio::test]
async fn test_two_users_stake_and_unstake_independently() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();

    let bob_stake = STAKE_AMOUNT.saturating_div(2);

    // Both users stake.
    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message(alice.id(), None, None::<&str>),
        )
        .await?;
    env.lst
        .stake(bob, bob_stake, stake_message(bob.id(), None, None::<&str>))
        .await?;

    // Locked balance grows by the sum of both stakes.
    assert_eq!(
        env.lst.near_balance().await?.locked,
        INIT_LOCK
            .saturating_add(STAKE_AMOUNT)
            .saturating_add(bob_stake)
    );
    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, STAKE_AMOUNT);
    assert_eq!(env.lst.ft_balance_of(bob.id()).await?, bob_stake);
    assert_eq!(
        env.lst.ft_total_supply().await?,
        STAKE_AMOUNT.saturating_add(bob_stake)
    );

    // Each user unstakes using a message keyed to their own receiver_id, so the
    // two entries in the unstake queue are distinct.
    let alice_unstake_msg = unstake_message(alice.id(), WithdrawTokens::Native);
    let bob_unstake_msg = unstake_message(bob.id(), WithdrawTokens::Native);

    env.lst
        .ft_transfer_call(alice, env.lst.id(), STAKE_AMOUNT, &alice_unstake_msg)
        .await?;
    env.lst
        .ft_transfer_call(bob, env.lst.id(), bob_stake, &bob_unstake_msg)
        .await?;

    assert_eq!(env.lst.ft_total_supply().await?, ZERO_AMOUNT);

    env.wait_unstake_cooldown().await?;

    assert_eq!(env.lst.near_balance().await?.locked, INIT_LOCK);

    // Each user withdraws their own entry independently.
    env.lst.withdraw(alice, &alice_unstake_msg).await?;
    env.lst.withdraw(bob, &bob_unstake_msg).await?;

    assert_eq!(
        alice
            .near_balance()
            .await?
            .total
            .saturating_add(NearToken::from_yoctonear(2)),
        INITIAL_BALANCE
    );
    assert_eq!(
        bob.near_balance()
            .await?
            .total
            .saturating_add(NearToken::from_yoctonear(2)),
        INITIAL_BALANCE
    );

    Ok(())
}
