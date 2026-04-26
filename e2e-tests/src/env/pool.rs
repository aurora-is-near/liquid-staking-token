use near_api::types::transaction::result::ExecutionSuccess;
use near_api::{Data, NearToken};
use near_sdk::serde::Serialize;
use near_sdk::serde_json;
use near_sdk::serde_json::Value;
use std::str::FromStr;

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
        args: impl Serialize,
    ) -> anyhow::Result<ExecutionSuccess>;
    async fn ping(&self) -> anyhow::Result<ExecutionSuccess>;
    async fn set_protocol_fee_bps(&self, bps: u16) -> anyhow::Result<ExecutionSuccess>;
    async fn get_reward_fee_fraction(&self) -> anyhow::Result<Value>;
    async fn get_exchange_rate(&self) -> anyhow::Result<f64>;
    async fn get_number_of_accounts(&self) -> anyhow::Result<u64>;
    async fn get_total_staked_balance(&self) -> anyhow::Result<NearToken>;
    async fn get_total_pending_withdrawals(&self) -> anyhow::Result<NearToken>;
    async fn get_total_balance(&self) -> anyhow::Result<NearToken>;
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

    async fn withdraw(
        &self,
        signer: &Account,
        args: impl Serialize,
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

    async fn ping(&self) -> anyhow::Result<ExecutionSuccess> {
        let result = self
            .inner
            .call_function("ping", serde_json::json!({}))
            .transaction()
            .with_signer(self.id().clone(), self.signer())
            .send_to(self.config())
            .await?;

        result.into_result().map_err(Into::into)
    }

    async fn set_protocol_fee_bps(&self, bps: u16) -> anyhow::Result<ExecutionSuccess> {
        let result = self
            .inner
            .call_function(
                "set_protocol_fee_bps",
                serde_json::json!({ "fee_bps": bps }),
            )
            .transaction()
            .with_signer(self.id().clone(), self.signer())
            .send_to(self.config())
            .await?;

        result.into_result().map_err(Into::into)
    }

    async fn get_reward_fee_fraction(&self) -> anyhow::Result<Value> {
        self.inner
            .call_function("get_reward_fee_fraction", ())
            .read_only()
            .fetch_from(self.config())
            .await
            .map(|fraction: Data<Value>| fraction.data)
            .map_err(Into::into)
    }

    async fn get_exchange_rate(&self) -> anyhow::Result<f64> {
        let result = self
            .inner
            .call_function("get_exchange_rate", ())
            .read_only()
            .fetch_from(self.config())
            .await
            .map(|fraction: Data<Value>| fraction.data)?;

        let n = result["numerator"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Failed to parse numerator from exchange rate"))?;
        let d = result["denominator"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Failed to parse denominator from exchange rate"))?;

        Ok(f64::from_str(n)? / f64::from_str(d)?)
    }

    async fn get_number_of_accounts(&self) -> anyhow::Result<u64> {
        self.inner
            .call_function("get_number_of_accounts", ())
            .read_only()
            .fetch_from(self.config())
            .await
            .map(|number: Data<u64>| number.data)
            .map_err(Into::into)
    }

    async fn get_total_staked_balance(&self) -> anyhow::Result<NearToken> {
        self.inner
            .call_function("get_total_staked_balance", ())
            .read_only()
            .fetch_from(self.config())
            .await
            .map(|balance: Data<NearToken>| balance.data)
            .map_err(Into::into)
    }

    async fn get_total_pending_withdrawals(&self) -> anyhow::Result<NearToken> {
        self.inner
            .call_function("get_total_pending_withdrawals", ())
            .read_only()
            .fetch_from(self.config())
            .await
            .map(|balance: Data<NearToken>| balance.data)
            .map_err(Into::into)
    }

    async fn get_total_balance(&self) -> anyhow::Result<NearToken> {
        self.inner
            .call_function("get_total_balance", ())
            .read_only()
            .fetch_from(self.config())
            .await
            .map(|balance: Data<NearToken>| balance.data)
            .map_err(Into::into)
    }
}
