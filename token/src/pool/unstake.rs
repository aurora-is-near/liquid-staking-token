use near_sdk::json_types::U128;
use near_sdk::{
    AccountId, CryptoHash, Gas, NearToken, Promise, PromiseOrValue, env, near, require,
};

use crate::pool::MODIFY_STATE_AFTER_STAKE_GAS;
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
    #[private]
    pub fn on_unstake(
        &mut self,
        lst_amount: NearToken,
        near_amount: NearToken,
        msg_hash: CryptoHash,
    ) -> PromiseOrValue<U128> {
        if env::promise_result_checked(0, 0).is_ok() {
            near_sdk::log!("Unstake successful");
            let epoch_id = env::epoch_height();
            let user_distribution = self.unstake_queue.entry(msg_hash).or_default();

            user_distribution.withdrawal_amount = user_distribution
                .withdrawal_amount
                .saturating_add(near_amount);
            user_distribution.unstake_epoch = epoch_id;

            self.statistics.increase_pending_withdrawals(near_amount);

            PromiseOrValue::Value(0.into())
        } else {
            let lst_yocto = lst_amount.as_yoctonear();
            near_sdk::log!("Error while unstaking, refund: {lst_yocto} LST");

            PromiseOrValue::Value(lst_yocto.into())
        }
    }
}

impl LiquidStakingToken {
    pub(crate) fn handle_unstaking(
        &mut self,
        lst_amount: NearToken,
        args: &UnstakeMessage,
    ) -> Promise {
        require!(
            lst_amount > NearToken::ZERO,
            "Unstake amount must be more than 0"
        );

        self.sync_rewards_internal(None);

        let msg_hash = args
            .hash()
            .unwrap_or_else(|_| env::panic_str("Failed to hash the message"));

        let unstake_amount = self.lst_to_near(lst_amount);

        require!(
            unstake_amount <= self.statistics.total_staked_amount,
            "Attempt to unstake more than staked"
        );

        let new_total_staked_amount = self
            .statistics
            .total_staked_amount
            .checked_sub(unstake_amount)
            .unwrap_or_else(|| {
                env::panic_str("Overflow while calculating new total staked amount")
            });

        Self::ext_on(
            Promise::new(env::current_account_id())
                .stake(new_total_staked_amount, self.validator_public_key.clone()),
        )
        .with_unused_gas_weight(0)
        .with_static_gas(MODIFY_STATE_AFTER_STAKE_GAS)
        .modify_state_after_stake(
            &env::current_account_id(),
            new_total_staked_amount,
            lst_amount,
            false,
        )
        .then(
            Self::ext(env::current_account_id())
                .with_unused_gas_weight(1)
                .with_static_gas(ON_UNSTAKE_GAS)
                .on_unstake(lst_amount, unstake_amount, msg_hash),
        )
    }
}
