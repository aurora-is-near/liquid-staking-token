use near_api::NearToken;
use near_api::types::transaction::result::ExecutionSuccess;
use near_sdk::serde::Serialize;
use near_sdk::serde_json;

use crate::env::types::{Account, Contract};

pub trait StakingPool {
    async fn stake(
        &self,
        signer: &Account,
        amount: NearToken,
        args: impl Serialize,
    ) -> anyhow::Result<ExecutionSuccess>;
    async fn withdraw(
        &self,
        signer: &Account,
        args: &serde_json::Value,
    ) -> anyhow::Result<ExecutionSuccess>;
}

impl StakingPool for Contract {
    async fn stake(
        &self,
        signer: &Account,
        amount: NearToken,
        args: impl Serialize,
    ) -> anyhow::Result<ExecutionSuccess> {
        self.inner
            .call_function(
                "stake",
                serde_json::json!({
                    "args": args,
                }),
            )
            .transaction()
            .deposit(amount)
            .max_gas()
            .with_signer(signer.id().clone(), signer.signer())
            .send_to(self.config())
            .await?
            .into_result()
            .map_err(Into::into)
    }

    // async fn unstake(
    //     &self,
    //     signer: &Account,
    //     amount: NearToken,
    // ) -> anyhow::Result<ExecutionSuccess> {
    //     self.inner
    //         .call_function(
    //             "unstake",
    //             serde_json::json!({
    //                 "amount": amount
    //             }),
    //         )
    //         .transaction()
    //         .with_signer(signer.id().clone(), signer.signer())
    //         .send_to(self.config())
    //         .await?
    //         .into_result()
    //         .map_err(Into::into)
    // }

    async fn withdraw(
        &self,
        signer: &Account,
        args: &serde_json::Value,
    ) -> anyhow::Result<ExecutionSuccess> {
        let result = self
            .inner
            .call_function(
                "withdraw",
                serde_json::json!({
                    "args": args,
                }),
            )
            .transaction()
            .max_gas()
            .with_signer(signer.id().clone(), signer.signer())
            .send_to(self.config())
            .await?;

        result.into_result().map_err(Into::into)
    }
}

// fn get_epoch_height_from_logs(logs: &[&str]) -> anyhow::Result<u64> {
//     let epoch_height_str = logs
//         .iter()
//         .find(|log| log.starts_with("Epoch "))
//         .ok_or_else(|| anyhow::anyhow!("Failed to find EpochHeight in logs: {logs:?}"))?;
//
//     epoch_height_str
//         .split_once(':')
//         .and_then(|(epoch_height_str, _)| epoch_height_str.split_once(' '))
//         .ok_or_else(|| anyhow::anyhow!("Failed to split EpochHeight from log: {epoch_height_str}"))
//         .and_then(|(_, epoch_height_str)| {
//             epoch_height_str
//                 .trim()
//                 .parse::<u64>()
//                 .map_err(|e| anyhow::anyhow!("Failed to parse EpochHeight: {e}"))
//         })
// }
