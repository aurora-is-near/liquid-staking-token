use liquid_staking_token::pool::WithdrawTokens;
use testresult::TestResult;

use crate::env::ft::FungibleToken;
use crate::env::mt::MultiToken;
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::env::{Env, INIT_BALANCE, INIT_LOCK};
use crate::tests::{
    ONE_YOCTO, STAKE_AMOUNT, ZERO_AMOUNT, stake_message_with_refund, unstake_message,
};

#[tokio::test]
async fn test_stake_with_native_near_and_to_send_on_intents_with_bad_account_with_intents_refund()
-> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();
    let alice_native_balance_before = alice.near_balance().await?;
    let refund_message = unstake_message(
        env.intents.id(),
        &WithdrawTokens::Wnear {
            is_storage_deposit: false,
            msg: Some(bob.id().to_string()),
            memo: None,
            min_gas: None,
        },
    );

    env.lst
        .stake(
            alice,
            STAKE_AMOUNT,
            stake_message_with_refund(
                env.intents.id(),
                false,
                Some("a2933a$$%$1!@!#@!@"),
                Some(&refund_message),
            ), // Triggers a panic in `ft_on_transfer` on intents.
        )
        .await?;

    assert_eq!(
        env.lst.get_total_balance().await?,
        INIT_BALANCE.saturating_add(STAKE_AMOUNT)
    );
    assert_eq!(env.lst.ft_balance_of(env.intents.id()).await?, ZERO_AMOUNT);
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
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
    );

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &refund_message).await?;

    let wnear_intents_balance = env.wnear.ft_balance_of(env.intents.id()).await?;
    assert_eq!(wnear_intents_balance, STAKE_AMOUNT);

    let bob_intents_balance = env.intents.mt_balance_of(bob.id(), env.wnear.id()).await?;
    assert_eq!(bob_intents_balance, STAKE_AMOUNT);

    Ok(())
}

#[tokio::test]
async fn test_stake_with_attempt_to_get_shared_tokens_on_contract() -> TestResult {
    let env = Env::builder().build().await?;
    let alice = env.alice();
    let bob = env.bob();
    let refund_message = unstake_message(
        env.intents.id(),
        &WithdrawTokens::Wnear {
            is_storage_deposit: false,
            msg: Some(alice.id().to_string()),
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

    // No tokens minted, total_supply unchanged.
    assert_eq!(env.lst.ft_total_supply().await?, INIT_LOCK);
    // Locked balance increased by STAKE_AMOUNT for cooldown period only.
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

    env.wait_unstake_cooldown().await?;

    env.lst.withdraw(alice, &refund_message).await?;

    assert_eq!(env.lst.near_balance().await?.locked, INIT_LOCK);

    assert_eq!(env.lst.ft_balance_of(alice.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        env.wnear.ft_balance_of(env.intents.id()).await?,
        STAKE_AMOUNT
    );
    assert_eq!(
        alice.near_balance().await?.total,
        INIT_BALANCE
            .saturating_sub(STAKE_AMOUNT)
            .saturating_sub(ONE_YOCTO)
    );
    assert_eq!(
        env.intents
            .mt_balance_of(alice.id(), env.wnear.id())
            .await?,
        STAKE_AMOUNT
    );

    assert_eq!(env.lst.ft_balance_of(bob.id()).await?, ZERO_AMOUNT);
    assert_eq!(
        bob.near_balance().await?.total,
        INIT_BALANCE.saturating_sub(ONE_YOCTO)
    );

    Ok(())
}
