use crate::env::Env;
use crate::env::ft::FungibleToken;
use crate::env::pool::StakingPool;
use crate::tests::{stake_message, unstake_message};
use liquid_staking_token::pool::WithdrawTokens;
use near_api::{NearGas, NearToken};
use near_sdk::serde_json::json;

#[tokio::test]
async fn security_low_gas_withdraw_keeps_claim_available() -> anyhow::Result<()> {
    let env = Env::builder().with_stake_rewards([0, 1]).build().await?;
    let alice = env.alice();
    let amount = NearToken::from_near(100);
    env.lst
        .stake(
            alice,
            amount,
            stake_message(alice.id(), false, None::<&str>),
        )
        .await?;
    let args = unstake_message(alice.id(), &WithdrawTokens::Native);
    let alice_lst = env.lst.ft_balance_of(alice.id()).await?;
    env.lst
        .ft_transfer_call(alice, env.lst.id(), alice_lst, &args)
        .await?;
    let claim = env.lst.get_total_pending_withdrawals().await?;
    env.wait_unstake_cooldown().await?;
    let result = env
        .lst
        .inner
        .call_function("withdraw", json!({"args": args}))
        .transaction()
        .gas(NearGas::from_tgas(9))
        .with_signer(alice.id().clone(), alice.signer())
        .send_to(env.lst.config())
        .await?;
    assert!(result.into_result().is_err());
    assert_eq!(env.lst.get_total_pending_withdrawals().await?, claim);
    assert!(
        env.lst
            .get_hashes_available_for_withdrawal(0, 10)
            .await?
            .contains(&args.hash()?)
    );
    env.lst.withdraw(alice, &args).await?;
    assert_eq!(
        env.lst.get_total_pending_withdrawals().await?,
        NearToken::ZERO
    );
    Ok(())
}
