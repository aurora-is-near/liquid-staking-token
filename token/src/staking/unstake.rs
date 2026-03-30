use near_sdk::json_types::U128;
use near_sdk::serde_json::json;
use near_sdk::{
    AccountId, CryptoHash, Gas, NearToken, Promise, PromiseOrValue, env, near, require,
};

use crate::staking::MODIFY_STAKED_AMOUNT_GAS;
use crate::{LiquidStakingToken, LiquidStakingTokenExt};

const ON_UNSTAKE_GAS: Gas = Gas::from_tgas(5);

#[derive(Debug, Clone)]
#[near(serializers = [json])]
#[serde(rename_all = "lowercase")]
pub enum WithdrawTokens {
    Native,
    Wnear {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        storage_deposit: Option<NearToken>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        msg: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        memo: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_gas: Option<Gas>,
    },
}

#[derive(Debug, Clone)]
#[near(serializers = [json])]
#[serde(rename_all = "lowercase")]
pub struct UnstakeMessage {
    /// The account ID to which the staked tokens should be sent.
    pub receiver_id: AccountId,
    /// Type of tokens to withdraw.
    pub withdraw_tokens: WithdrawTokens,
}

impl UnstakeMessage {
    /// Computes a cryptographic hash of the stake message.
    ///
    /// This method serializes the `StakeMessage` to JSON format and then applies
    /// the Keccak-256 hashing algorithm to produce a unique hash value.
    ///
    /// # Returns
    ///
    /// Returns `Ok(CryptoHash)` containing the Keccak-256 hash of the serialized message,
    /// or `Err` if the serialization to JSON fails.
    ///
    /// # Errors
    ///
    /// Returns a `near_sdk::serde_json::Error` if the stake message cannot be serialized to JSON.
    pub fn hash(&self) -> Result<CryptoHash, near_sdk::serde_json::Error> {
        near_sdk::serde_json::to_vec(self).map(env::keccak256_array)
    }
}

#[near]
impl LiquidStakingToken {
    pub(crate) fn handle_unstaking(
        &self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        let args: UnstakeMessage = near_sdk::serde_json::from_str(&msg)
            .unwrap_or_else(|_| env::panic_str("Invalid msg format"));
        let msg_hash = args
            .hash()
            .unwrap_or_else(|_| env::panic_str("Failed to hash the message"));

        let unstake_amount = NearToken::from_yoctonear(amount.0);

        require!(
            unstake_amount <= self.total_staked_amount,
            "Attempt to unstake more than staked"
        );

        let new_staked_amount = self
            .total_staked_amount
            .checked_sub(unstake_amount)
            .unwrap_or_else(|| env::panic_str("Attempt to unstake more than a locked balance"));

        Promise::new(env::current_account_id())
            .stake(new_staked_amount, self.validator_public_key.clone())
            .function_call(
                "modify_total_staked_amount",
                json!({
                    "amount": new_staked_amount,
                })
                .to_string()
                .into_bytes(),
                NearToken::ZERO,
                MODIFY_STAKED_AMOUNT_GAS,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_unused_gas_weight(1)
                    .with_static_gas(ON_UNSTAKE_GAS)
                    .on_unstake(sender_id, amount, msg_hash),
            )
            .into()
    }

    #[private]
    pub fn on_unstake(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg_hash: CryptoHash,
    ) -> PromiseOrValue<U128> {
        let _ = sender_id;
        match env::promise_result_checked(0, 0) {
            Ok(_) => {
                near_sdk::log!("Unstake successful");
                let epoch_id = env::epoch_height();
                self.token
                    .internal_withdraw(&env::current_account_id(), amount.0);
                let (unstake_amount, unstake_epoch) =
                    self.unstake_queue.entry(msg_hash).or_insert((0, epoch_id));

                *unstake_amount = unstake_amount.saturating_add(amount.0);
                *unstake_epoch = epoch_id;

                PromiseOrValue::Value(0.into())
            }
            Err(e) => {
                near_sdk::log!("Error while untaking: {e}");
                PromiseOrValue::Value(amount)
            }
        }
    }
}
