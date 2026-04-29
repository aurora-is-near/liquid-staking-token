use liquid_staking_token::ONE_YOCTO;
use near_api::types::json::U128;
use near_api::types::storage::StorageBalance;
use near_api::types::transaction::result::ExecutionSuccess;
use near_api::{AccountId, Data, NearToken, Tokens};
use near_sdk::serde_json::json;

use crate::env::types::{Account, Contract};

pub const FT_STORAGE_DEPOSIT: NearToken = NearToken::from_micronear(1250);

pub trait FungibleToken {
    async fn ft_balance_of(&self, account_id: &AccountId) -> anyhow::Result<NearToken>;
    async fn ft_total_supply(&self) -> anyhow::Result<NearToken>;
    async fn ft_storage_balance_of(
        &self,
        account_id: &AccountId,
    ) -> anyhow::Result<Option<StorageBalance>>;
    async fn ft_transfer(
        &self,
        sender: &Account,
        receiver_id: &AccountId,
        amount: NearToken,
    ) -> anyhow::Result<ExecutionSuccess>;
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
    async fn ft_storage_deposit(
        &self,
        signer: &Account,
        account_id: &AccountId,
    ) -> anyhow::Result<ExecutionSuccess>;
    async fn ft_storage_deposit_with_amount(
        &self,
        signer: &Account,
        account_id: &AccountId,
        amount: NearToken,
        registration_only: Option<bool>,
    ) -> anyhow::Result<ExecutionSuccess>;
    async fn ft_storage_withdraw(
        &self,
        signer: &Account,
        amount: Option<NearToken>,
    ) -> anyhow::Result<ExecutionSuccess>;
    async fn ft_storage_unregister(
        &self,
        signer: &Account,
        force: Option<bool>,
    ) -> anyhow::Result<ExecutionSuccess>;
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

    async fn ft_storage_balance_of(
        &self,
        account_id: &AccountId,
    ) -> anyhow::Result<Option<StorageBalance>> {
        self.inner
            .storage_deposit()
            .view_account_storage(account_id.clone())
            .fetch_from(self.config())
            .await
            .map(|storage: Data<Option<StorageBalance>>| storage.data)
            .map_err(Into::into)
    }

    async fn ft_transfer(
        &self,
        sender: &Account,
        receiver_id: &AccountId,
        amount: NearToken,
    ) -> anyhow::Result<ExecutionSuccess> {
        self.inner
            .call_function(
                "ft_transfer",
                json!({
                    "receiver_id": receiver_id,
                    "amount": amount,
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

    async fn ft_storage_deposit(
        &self,
        signer: &Account,
        account_id: &AccountId,
    ) -> anyhow::Result<ExecutionSuccess> {
        self.ft_storage_deposit_with_amount(signer, account_id, FT_STORAGE_DEPOSIT, Some(true))
            .await
    }

    async fn ft_storage_deposit_with_amount(
        &self,
        signer: &Account,
        account_id: &AccountId,
        amount: NearToken,
        registration_only: Option<bool>,
    ) -> anyhow::Result<ExecutionSuccess> {
        let args = registration_only.map_or_else(
            || {
                json!({
                    "account_id": account_id,
                })
            },
            |registration_only| {
                json!({
                    "account_id": account_id,
                    "registration_only": registration_only,
                })
            },
        );

        self.inner
            .call_function("storage_deposit", args)
            .transaction()
            .deposit(amount)
            .max_gas()
            .with_signer(signer.id().clone(), signer.signer())
            .send_to(signer.config())
            .await?
            .into_result()
            .map_err(Into::into)
    }

    async fn ft_storage_withdraw(
        &self,
        signer: &Account,
        amount: Option<NearToken>,
    ) -> anyhow::Result<ExecutionSuccess> {
        self.inner
            .call_function(
                "storage_withdraw",
                json!({
                    "amount": amount,
                }),
            )
            .transaction()
            .deposit(ONE_YOCTO)
            .max_gas()
            .with_signer(signer.id().clone(), signer.signer())
            .send_to(signer.config())
            .await?
            .into_result()
            .map_err(Into::into)
    }

    async fn ft_storage_unregister(
        &self,
        signer: &Account,
        force: Option<bool>,
    ) -> anyhow::Result<ExecutionSuccess> {
        self.inner
            .call_function("storage_unregister", json!({"force": force}))
            .transaction()
            .deposit(ONE_YOCTO)
            .max_gas()
            .with_signer(signer.id().clone(), signer.signer())
            .send_to(signer.config())
            .await?
            .into_result()
            .map_err(Into::into)
    }
}
