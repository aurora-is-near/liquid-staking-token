use near_api::types::Action;
use near_api::types::transaction::actions::FunctionCallAction;
use near_api::{NearGas, NearToken, Transaction};
use near_sdk::serde_json::json;

use crate::env::Env;
use crate::env::ft::{FT_STORAGE_DEPOSIT, FungibleToken};
use crate::env::native::Native;
use crate::env::pool::StakingPool;
use crate::tests::assertions::assert_storage_balance;

#[tokio::test]
async fn security_batched_storage_deposit_refunds_do_not_count_as_rewards() -> anyhow::Result<()> {
    let env = Env::builder()
        .without_inflation()
        .without_storage_deposit()
        .build()
        .await?;
    env.lst.set_protocol_fee_bps(1000).await?;
    // Setting the fee can sync its attached yoctoNEAR. Advance so the epoch
    // guard cannot hide phantom rewards from the batched storage refund.
    env.wait_for_next_epoch_start().await?;

    // Each account is first registered, then receives a full deposit refund.
    for (user, registered, registration_only) in [
        (env.alice(), false, true),
        (env.alice(), true, true),
        (env.bob(), false, false),
        (env.bob(), true, false),
    ] {
        let case = format!("registered={registered}, registration_only={registration_only}");
        assert_eq!(
            env.lst.ft_storage_balance_of(user.id()).await?.is_some(),
            registered,
            "{case}"
        );
        let retained = if registered {
            NearToken::ZERO
        } else {
            FT_STORAGE_DEPOSIT
        };
        let before_tracker = env.lst.get_total_balance().await?;
        let before_staked = env.lst.get_total_staked_balance().await?;
        let before_supply = env.lst.ft_total_supply().await?;
        let before_treasury = env.lst.ft_balance_of(env.lst.id()).await?;
        let before_user = user.near_balance().await?.total;

        // Both calls must be actions of one receipt: the refund has been
        // scheduled, but its recipient has not received it when ping runs.
        let actions = vec![
            Action::FunctionCall(Box::new(FunctionCallAction {
                method_name: "storage_deposit".to_owned(),
                args: near_sdk::serde_json::to_vec(&json!({
                    "account_id": user.id(),
                    "registration_only": registration_only,
                }))?,
                gas: NearGas::from_tgas(30),
                deposit: NearToken::from_near(1000),
            })),
            Action::FunctionCall(Box::new(FunctionCallAction {
                method_name: "ping".to_owned(),
                args: b"{}".to_vec(),
                gas: NearGas::from_tgas(100),
                deposit: NearToken::ZERO,
            })),
        ];
        let result = Transaction::construct(user.id().clone(), env.lst.id().clone())
            .add_actions(actions)
            .with_signer(user.signer())
            .send_to(env.lst.config())
            .await?
            .into_result()?;
        assert!(
            result.receipt_failures().is_empty(),
            "{case}: {:?}",
            result.receipt_failures()
        );

        // Unchanged backing and supply also preserve the exact exchange rate.
        assert_eq!(
            env.lst.get_total_staked_balance().await?,
            before_staked,
            "{case}"
        );
        assert_eq!(env.lst.ft_total_supply().await?, before_supply, "{case}");
        assert_eq!(
            env.lst.ft_balance_of(env.lst.id()).await?,
            before_treasury,
            "{case}"
        );
        assert_eq!(
            env.lst.get_total_balance().await?,
            before_tracker.saturating_add(retained),
            "{case}"
        );
        assert_eq!(
            user.near_balance().await?.total,
            before_user.saturating_sub(retained),
            "{case}"
        );
        assert_storage_balance(
            env.lst.ft_storage_balance_of(user.id()).await?,
            FT_STORAGE_DEPOSIT,
            NearToken::ZERO,
        );
        let balance = env.lst.near_balance().await?;
        assert_eq!(
            balance.total.saturating_add(balance.locked),
            env.lst.get_total_balance().await?,
            "{case}"
        );
        let followup = env.lst.ping().await?;
        assert!(
            followup.receipt_failures().is_empty(),
            "{case}: {:?}",
            followup.receipt_failures()
        );
    }
    Ok(())
}
