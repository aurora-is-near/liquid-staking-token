use near_api::types::json::U128;
use near_api::types::transaction::result::ExecutionSuccess;
use near_api::{AccountId, Data, NearToken, Tokens};
use near_sdk::serde_json::json;

use crate::env::types::{Account, Contract};

pub const FT_STORAGE_DEPOSIT: NearToken = NearToken::from_micronear(1250);

pub trait FungibleToken {
    async fn ft_balance_of(&self, account_id: &AccountId) -> anyhow::Result<NearToken>;
    async fn ft_total_supply(&self) -> anyhow::Result<NearToken>;
    async fn ft_transfer_call(
        &self,
        sender: &Account,
        receiver_id: &AccountId,
        amount: NearToken,
        msg: impl ToString,
    ) -> anyhow::Result<ExecutionSuccess>;
    async fn ft_on_transfer(
        &self,
        sender: &Account,
        sender_id: &AccountId,
        amount: NearToken,
        msg: impl ToString,
    ) -> anyhow::Result<ExecutionSuccess>;
    async fn ft_storage_deposit(&self, account_id: &AccountId) -> anyhow::Result<()>;
}

impl FungibleToken for Contract {
    async fn ft_balance_of(&self, account_id: &AccountId) -> anyhow::Result<NearToken> {
        Tokens::account(account_id.clone())
            .ft_balance(self.id().clone())
            .fetch_from(self.config())
            .await
            .map(|balance| NearToken::from_yoctonear(balance.amount()))
            .map_err(Into::into)
    }

    async fn ft_total_supply(&self) -> anyhow::Result<NearToken> {
        self.inner
            .call_function("ft_total_supply", json!({}))
            .read_only()
            .fetch_from(self.config())
            .await
            .map(|supply: Data<U128>| NearToken::from_yoctonear(supply.data.0))
            .map_err(Into::into)
    }

    async fn ft_transfer_call(
        &self,
        sender: &Account,
        receiver_id: &AccountId,
        amount: NearToken,
        msg: impl ToString,
    ) -> anyhow::Result<ExecutionSuccess> {
        self.inner
            .call_function(
                "ft_transfer_call",
                json!({
                    "receiver_id": receiver_id,
                    "amount": amount,
                    "msg": msg.to_string(),
                }),
            )
            .transaction()
            .deposit(NearToken::from_yoctonear(1))
            .max_gas()
            .with_signer(sender.id().clone(), sender.signer())
            .send_to(self.config())
            .await?
            .into_result()
            .map_err(Into::into)
    }

    async fn ft_on_transfer(
        &self,
        sender: &Account,
        sender_id: &AccountId,
        amount: NearToken,
        msg: impl ToString,
    ) -> anyhow::Result<ExecutionSuccess> {
        self.inner
            .call_function(
                "ft_on_transfer",
                json!({
                    "sender_id": sender_id,
                    "amount": amount,
                    "msg": msg.to_string(),
                }),
            )
            .transaction()
            .max_gas()
            .with_signer(sender.id().clone(), sender.signer())
            .send_to(self.config())
            .await?
            .into_result()
            .map_err(Into::into)
    }

    async fn ft_storage_deposit(&self, account_id: &AccountId) -> anyhow::Result<()> {
        self.inner
            .storage_deposit()
            .deposit(account_id.clone(), FT_STORAGE_DEPOSIT)
            .registration_only()
            .with_signer(self.id().clone(), self.signer())
            .send_to(self.config())
            .await?
            .assert_success();

        Ok(())
    }
}
