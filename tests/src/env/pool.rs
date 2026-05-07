#![allow(dead_code)]
use liquid_staking_token::ONE_YOCTO;
use liquid_staking_token::pool::{Tranche, WithdrawalRequest};
use near_api::types::transaction::result::ExecutionSuccess;
use near_api::{Data, NearToken, PublicKey};
use near_sdk::CryptoHash;
use near_sdk::serde::Serialize;
use near_sdk::serde_json::{Value, json};
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
    async fn set_validator_public_key(
        &self,
        validator_public_key: PublicKey,
    ) -> anyhow::Result<ExecutionSuccess>;
    async fn force_release_lock(&mut self, hash: CryptoHash) -> anyhow::Result<bool>;
    async fn get_reward_fee_fraction(&self) -> anyhow::Result<Value>;
    async fn get_exchange_rate(&self) -> anyhow::Result<f64>;
    async fn get_number_of_accounts(&self) -> anyhow::Result<u64>;
    async fn get_total_staked_balance(&self) -> anyhow::Result<NearToken>;
    async fn get_total_pending_withdrawals(&self) -> anyhow::Result<NearToken>;
    async fn get_total_balance(&self) -> anyhow::Result<NearToken>;
    async fn get_withdrawal_request_tranches(
        &self,
        hash: CryptoHash,
    ) -> anyhow::Result<Option<Vec<Tranche>>>;
    async fn get_withdrawal_requests(
        &self,
        skip: usize,
        limit: usize,
    ) -> anyhow::Result<Vec<WithdrawalRequest>>;
    async fn get_hashes_available_for_withdrawal(
        &self,
        skip: usize,
        limit: usize,
    ) -> anyhow::Result<Vec<CryptoHash>>;
    async fn get_withdrawal_requests_count(&self) -> anyhow::Result<u32>;
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
                json!({
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
        self.inner
            .call_function("withdraw", json!({ "args": args }))
            .transaction()
            .max_gas()
            .with_signer(signer.id().clone(), signer.signer())
            .send_to(self.config())
            .await?
            .into_result()
            .map_err(Into::into)
    }

    async fn ping(&self) -> anyhow::Result<ExecutionSuccess> {
        self.inner
            .call_function("ping", ())
            .transaction()
            .with_signer(self.id().clone(), self.signer())
            .send_to(self.config())
            .await?
            .into_result()
            .map_err(Into::into)
    }

    async fn set_protocol_fee_bps(&self, bps: u16) -> anyhow::Result<ExecutionSuccess> {
        self.inner
            .call_function("set_protocol_fee_bps", json!({ "fee_bps": bps }))
            .transaction()
            .deposit(ONE_YOCTO)
            .with_signer(self.id().clone(), self.signer())
            .send_to(self.config())
            .await?
            .into_result()
            .map_err(Into::into)
    }

    async fn set_validator_public_key(
        &self,
        validator_public_key: PublicKey,
    ) -> anyhow::Result<ExecutionSuccess> {
        self.inner
            .call_function(
                "set_validator_public_key",
                json!({ "validator_public_key": validator_public_key }),
            )
            .transaction()
            .deposit(ONE_YOCTO)
            .with_signer(self.id().clone(), self.signer())
            .send_to(self.config())
            .await?
            .into_result()
            .map_err(Into::into)
    }

    async fn force_release_lock(&mut self, hash: CryptoHash) -> anyhow::Result<bool> {
        self.inner
            .call_function("force_release_lock", json!({ "hash": hash }))
            .transaction()
            .deposit(ONE_YOCTO)
            .with_signer(self.id().clone(), self.signer())
            .send_to(self.config())
            .await?
            .into_result()?
            .json()
            .map_err(Into::into)
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

    async fn get_withdrawal_request_tranches(
        &self,
        hash: CryptoHash,
    ) -> anyhow::Result<Option<Vec<Tranche>>> {
        self.inner
            .call_function("get_withdrawal_request_tranches", json!({ "hash": hash }))
            .read_only()
            .fetch_from(self.config())
            .await
            .map(|tranches: Data<Option<Vec<Tranche>>>| tranches.data)
            .map_err(Into::into)
    }

    async fn get_withdrawal_requests(
        &self,
        skip: usize,
        limit: usize,
    ) -> anyhow::Result<Vec<WithdrawalRequest>> {
        self.inner
            .call_function(
                "get_withdrawal_requests",
                json!({"skip": skip, "limit": limit}),
            )
            .read_only()
            .fetch_from(self.config())
            .await
            .map(|requests: Data<Vec<WithdrawalRequest>>| requests.data)
            .map_err(Into::into)
    }

    async fn get_hashes_available_for_withdrawal(
        &self,
        skip: usize,
        limit: usize,
    ) -> anyhow::Result<Vec<CryptoHash>> {
        self.inner
            .call_function(
                "get_hashes_available_for_withdrawal",
                json!({"skip": skip, "limit": limit}),
            )
            .read_only()
            .fetch_from(self.config())
            .await
            .map(|hashes: Data<Vec<CryptoHash>>| hashes.data)
            .map_err(Into::into)
    }

    async fn get_withdrawal_requests_count(&self) -> anyhow::Result<u32> {
        self.inner
            .call_function("get_withdrawal_requests_count", ())
            .read_only()
            .fetch_from(self.config())
            .await
            .map(|count: Data<u32>| count.data)
            .map_err(Into::into)
    }
}
