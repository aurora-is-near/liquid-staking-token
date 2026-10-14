use crate::env::Env;
use crate::env::ft::{FtMessage, FungibleToken};
use crate::env::pool::StakingPool;
use crate::env::wnear::WNear;
use crate::tests::{ONE_YOCTO, stake_message};
use near_api::types::Action;
use near_api::types::transaction::actions::FunctionCallAction;
use near_api::{NearGas, NearToken, Transaction};
use near_sdk::serde_json::json;

#[tokio::test]
async fn security_batched_native_stakes_do_not_count_pending_deposits_as_rewards()
-> anyhow::Result<()> {
    let env = Env::builder().without_inflation().build().await?;
    let alice = env.alice();
    let amount = NearToken::from_near(100);
    let before_staked = env.lst.get_total_staked_balance().await?;
    let actions = (0..2)
        .map(|_| {
            Ok(Action::FunctionCall(Box::new(FunctionCallAction {
                method_name: "stake".to_owned(),
                args: near_sdk::serde_json::to_vec(
                    &json!({"args": stake_message(alice.id(), false, None::<&str>)}),
                )?,
                gas: NearGas::from_tgas(130),
                deposit: amount,
            })))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let result = Transaction::construct(alice.id().clone(), env.lst.id().clone())
        .add_actions(actions)
        .with_signer(alice.signer())
        .send_to(env.lst.config())
        .await?
        .into_result()?;
    assert!(
        result.receipt_failures().is_empty(),
        "{:?}",
        result.receipt_failures()
    );
    assert_eq!(
        env.lst.get_total_staked_balance().await?,
        before_staked.saturating_add(amount.saturating_mul(2))
    );
    Ok(())
}

#[tokio::test]
async fn security_batched_wnear_stakes_do_not_count_pending_deposits_as_rewards()
-> anyhow::Result<()> {
    let env = Env::builder().without_inflation().build().await?;
    let alice = env.alice();
    let amount = NearToken::from_near(100);
    let before_staked = env.lst.get_total_staked_balance().await?;
    env.wnear
        .near_deposit(alice, amount.saturating_mul(2))
        .await?;
    let actions = (0..2)
        .map(|_| {
            Ok(Action::FunctionCall(Box::new(FunctionCallAction {
                method_name: "ft_transfer_call".to_owned(),
                args: near_sdk::serde_json::to_vec(&json!({
                    "receiver_id": env.lst.id(), "amount": amount,
                    "msg": stake_message(alice.id(), false, None::<&str>).to_msg()?,
                }))?,
                gas: NearGas::from_tgas(150),
                deposit: ONE_YOCTO,
            })))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let result = Transaction::construct(alice.id().clone(), env.wnear.id().clone())
        .add_actions(actions)
        .with_signer(alice.signer())
        .send_to(env.wnear.config())
        .await?
        .into_result()?;
    assert!(
        result.receipt_failures().is_empty(),
        "{:?}",
        result.receipt_failures()
    );
    assert_eq!(env.wnear.ft_balance_of(alice.id()).await?, NearToken::ZERO);
    assert_eq!(
        env.lst.get_total_staked_balance().await?,
        before_staked.saturating_add(amount.saturating_mul(2))
    );
    Ok(())
}
