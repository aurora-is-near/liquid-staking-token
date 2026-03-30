use defuse::core::intents::DefuseIntents;
use defuse::core::intents::tokens::FtWithdraw;
use defuse::core::nep413::{Nep413Payload, SignedNep413Payload};
use defuse::core::payload::multi::MultiPayload;
use defuse::core::payload::nep413::Nep413DefuseMessage;
use defuse::core::{Deadline, Nonce};
use near_api::signer::NEP413Payload;
use near_api::types::transaction::result::ExecutionSuccess;
use near_api::{AccountId, NearToken, PublicKey};
use near_sdk::serde::Serialize;

use crate::env::signer;
use crate::env::types::{Account, Contract};

pub trait Defuse {
    async fn execute_intents(
        &self,
        sender_id: &AccountId,
        intents: Vec<MultiPayload>,
    ) -> anyhow::Result<ExecutionSuccess>;
    async fn add_public_key(
        &self,
        defuse_contract_id: &AccountId,
        public_key: PublicKey,
    ) -> anyhow::Result<()>;
}

impl Defuse for Contract {
    async fn execute_intents(
        &self,
        sender_id: &AccountId,
        intents: Vec<MultiPayload>,
    ) -> anyhow::Result<ExecutionSuccess> {
        self.inner
            .call_function(
                "execute_intents",
                near_sdk::serde_json::json!({
                    "signed": intents,
                }),
            )
            .transaction()
            .max_gas()
            .with_signer(sender_id.clone(), self.signer())
            .send_to(self.config())
            .await?
            .into_result()
            .map_err(Into::into)
    }

    async fn add_public_key(
        &self,
        account_id: &AccountId,
        public_key: PublicKey,
    ) -> anyhow::Result<()> {
        self.inner
            .call_function(
                "add_public_key",
                near_sdk::serde_json::json!({
                    "public_key": public_key,
                }),
            )
            .transaction()
            .deposit(NearToken::from_yoctonear(1))
            .with_signer(account_id.clone(), self.signer())
            .send_to(self.config())
            .await?
            .assert_success();

        Ok(())
    }
}

pub trait DefuseSigner: Signer {
    #[must_use]
    async fn sign_defuse_message<T>(
        &self,
        defuse_contract: &AccountId,
        nonce: Nonce,
        deadline: Deadline,
        message: T,
    ) -> MultiPayload
    where
        T: Serialize;

    async fn sign_withdraw_intent(
        &self,
        defuse_contract: &AccountId,
        token: &AccountId,
        receiver_id: &AccountId,
        amount: NearToken,
        msg: Option<impl ToString>,
    ) -> MultiPayload {
        self.sign_defuse_message(
            defuse_contract,
            rand::random(),
            Deadline::MAX,
            DefuseIntents {
                intents: vec![
                    FtWithdraw {
                        token: token.clone(),
                        receiver_id: receiver_id.clone(),
                        amount: amount.as_yoctonear().into(),
                        memo: None,
                        msg: msg.map(|m| m.to_string()),
                        storage_deposit: None,
                        min_gas: None,
                    }
                    .into(),
                ],
            },
        )
        .await
    }
}

impl DefuseSigner for Account {
    async fn sign_defuse_message<T>(
        &self,
        defuse_contract: &AccountId,
        nonce: Nonce,
        deadline: Deadline,
        message: T,
    ) -> MultiPayload
    where
        T: Serialize,
    {
        self.sign_nep413(
            Nep413Payload::new(
                near_sdk::serde_json::to_string(&Nep413DefuseMessage {
                    signer_id: self.id().clone(),
                    deadline,
                    message,
                })
                .expect("Failed to serialize Nep413DefuseMessage message"),
            )
            .with_recipient(defuse_contract)
            .with_nonce(nonce),
        )
        .await
        .into()
    }
}

pub trait Signer {
    async fn sign_nep413(&self, payload: Nep413Payload) -> SignedNep413Payload;
}

impl Signer for Account {
    async fn sign_nep413(&self, payload: Nep413Payload) -> SignedNep413Payload {
        let public_key = signer()
            .get_public_key()
            .await
            .expect("Failed to get public key from signer");
        let signature = signer()
            .sign_message_nep413(
                self.id().clone(),
                public_key,
                &NEP413Payload {
                    message: payload.message.clone(),
                    nonce: payload.nonce,
                    recipient: payload.recipient.clone(),
                    callback_url: payload.callback_url.clone(),
                },
            )
            .await
            .unwrap();

        match (signature, public_key) {
            (near_api::types::Signature::ED25519(sig), PublicKey::ED25519(pk)) => {
                SignedNep413Payload {
                    payload,
                    public_key: pk.0,
                    signature: sig.to_bytes(),
                }
            }
            _ => unreachable!(),
        }
    }
}
