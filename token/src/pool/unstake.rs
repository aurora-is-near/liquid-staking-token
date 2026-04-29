use near_sdk::json_types::U128;
use near_sdk::{
    AccountId, CryptoHash, Gas, NearToken, Promise, PromiseOrValue, env, near, require,
};

use crate::pool::MODIFY_STATE_AFTER_STAKE_GAS;
use crate::{LiquidStakingToken, LiquidStakingTokenExt};

const ON_UNSTAKE_GAS: Gas = Gas::from_tgas(5);

/// Specifies the type of token to withdraw and associated withdrawal parameters.
///
/// This enum is used to distinguish between native NEAR token withdrawals and
/// wrapped NEAR (wNEAR) token withdrawals, where wNEAR withdrawals may require
/// additional configuration such as storage deposits and gas limits.
///
/// # Variants
///
/// * `Native` - Represents a withdrawal of native NEAR tokens. This is the simplest
///   form of withdrawal with no additional parameters.
///
/// * `Wnear` - Represents a withdrawal of wrapped NEAR tokens with optional
///   configuration parameters:
///   - `storage_deposit`: Optional storage deposit amount required for the wNEAR
///     contract interaction. If `None`, no storage deposit will be made.
///   - `msg`: Optional message string that can be passed to the wNEAR contract
///     during withdrawal. Omitted from serialization if `None`.
///   - `memo`: Optional memo string for additional transaction metadata or notes.
///     Omitted from serialization if `None`.
///   - `min_gas`: Optional minimum gas amount to attach to the wNEAR withdrawal
///     transaction. If `None`, a default gas amount will be used.
///
/// # Serialization
///
/// The enum uses lowercase variant names when serialized (via `#[serde(rename_all = "lowercase")]`).
/// Optional fields are skipped during serialization when they contain `None` values.
///
/// Supports both JSON and Borsh serialization formats for NEAR blockchain compatibility.
#[derive(Debug, Clone)]
#[near(serializers = [json, borsh])]
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

/// Represents a message for unstaking tokens and specifying withdrawal parameters.
///
/// This structure is used to define the details of an unstake operation, including
/// the recipient of the unstaked tokens and the type of tokens to be withdrawn.
///
/// # Fields
///
/// * `receiver_id` - The NEAR account ID that will receive the unstaked tokens.
///   This is the destination account where the tokens will be transferred after
///   the unstaking process is complete.
///
/// * `withdraw_tokens` - Specifies which type of tokens should be withdrawn during
///   the unstaking operation. See [`WithdrawTokens`] for available token types.
///
/// # Serialization
///
/// This structure supports both JSON and Borsh serialization formats through the
/// `#[near(serializers = [json, borsh])]` attribute, making it compatible with
/// NEAR protocol's storage and cross-contract communication requirements.
///
/// When serialized to JSON, field names are converted to lowercase via the
/// `#[serde(rename_all = "lowercase")]` attribute.
///
/// # Examples
///
/// ```ignore
/// use near_sdk::AccountId;
///
/// let unstake_msg = UnstakeMessage {
///     receiver_id: "alice.near".parse().unwrap(),
///     withdraw_tokens: WithdrawTokens::Native,
/// };
/// ```
#[derive(Debug, Clone)]
#[near(serializers = [json, borsh])]
#[serde(rename_all = "lowercase")]
pub struct UnstakeMessage {
    /// The account ID to which the staked tokens should be sent.
    pub receiver_id: AccountId,
    /// Type of tokens to withdraw.
    pub withdraw_tokens: WithdrawTokens,
}

impl UnstakeMessage {
    /// Computes the cryptographic hash of this object using the Keccak-256 algorithm.
    ///
    /// This method serializes the object using Borsh serialization and then applies
    /// the Keccak-256 hashing algorithm to produce a 32-byte hash value.
    ///
    /// # Returns
    ///
    /// * `Ok(CryptoHash)` - A 32-byte cryptographic hash of the serialized object
    ///
    /// # Errors
    ///
    ///  `Err(std::io::Error)` if the object cannot be serialized in borsh format.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let hash = my_object.hash()?;
    /// ```
    ///
    /// # Notes
    ///
    /// - The hash is deterministic: the same object will always produce the same hash
    /// - Uses Keccak-256, which is the same hashing algorithm used by Ethereum
    /// - The object must implement the `BorshSerialize` trait for this to work
    pub fn hash(&self) -> Result<CryptoHash, std::io::Error> {
        near_sdk::borsh::to_vec(self).map(env::keccak256_array)
    }
}

/// Represents the trigger for an unstaking process.
#[derive(Debug, Clone)]
#[near(serializers = [json])]
pub enum UnstakeTrigger {
    /// The unstake was initiated by the user.
    UserRequest,
    /// The unstake was triggered because of a refund by the contract.
    RefundByContract,
}

#[near]
impl LiquidStakingToken {
    #[private]
    pub fn on_unstake(
        &mut self,
        lst_amount: NearToken,
        near_amount: NearToken,
        msg_hash: CryptoHash,
        unstake_trigger: UnstakeTrigger,
    ) -> PromiseOrValue<U128> {
        if env::promise_result_checked(0, 0).is_ok() {
            near_sdk::log!("Unstake successful");
            let epoch_id = env::epoch_height();
            let user_distribution = self.unstake_queue.entry(msg_hash).or_default();

            user_distribution.withdrawal_amount = user_distribution
                .withdrawal_amount
                .checked_add(near_amount)
                .unwrap_or_else(|| env::panic_str("Overflow while increasing withdrawal amount"));
            // It's done intentionally. Each subsequent unstaking shifts the withdrawal epoch_id by 4 epochs.
            user_distribution.unstake_epoch = epoch_id;

            self.statistics.increase_pending_withdrawals(near_amount);

            PromiseOrValue::Value(0.into())
        } else {
            match unstake_trigger {
                UnstakeTrigger::UserRequest => {
                    let lst_yocto = lst_amount.as_yoctonear();
                    near_sdk::log!(
                        "Error while unstaking by user request, refund: {lst_yocto} LST"
                    );
                    PromiseOrValue::Value(lst_yocto.into())
                }
                UnstakeTrigger::RefundByContract => {
                    near_sdk::log!(
                        "Error while unstaking because of refund of LST tokens from staking"
                    );
                    PromiseOrValue::Value(0.into())
                }
            }
        }
    }

    #[private]
    pub fn modify_state_after_unstake(
        &mut self,
        unstake_amount: NearToken,
        lst_tokens: NearToken,
    ) -> Promise {
        self.statistics.total_staked_amount = self
            .statistics
            .total_staked_amount
            .checked_sub(unstake_amount)
            .unwrap_or_else(|| env::panic_str("Overflow while decreasing total staked amount"));
        self.internal_withdraw(&env::current_account_id(), lst_tokens);

        Promise::new(env::current_account_id()).stake(
            self.statistics.total_staked_amount,
            self.validator_public_key.clone(),
        )
    }
}

impl LiquidStakingToken {
    pub(crate) fn handle_unstaking(
        &mut self,
        lst_amount: NearToken,
        args: &UnstakeMessage,
        unstake_trigger: UnstakeTrigger,
    ) -> Promise {
        require!(
            lst_amount > NearToken::ZERO,
            "Unstake amount must be more than 0"
        );

        self.sync_rewards_internal(None);

        let unstake_amount = self.lst_to_near(lst_amount);

        require!(
            unstake_amount > NearToken::ZERO,
            "Unstake amount in NEAR must be more than 0"
        );

        require!(
            unstake_amount <= self.statistics.total_staked_amount,
            "Attempt to unstake more than staked"
        );

        let msg_hash = args
            .hash()
            .unwrap_or_else(|_| env::panic_str("Failed to hash the message"));

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
        .modify_state_after_unstake(unstake_amount, lst_amount)
        .then(
            Self::ext(env::current_account_id())
                .with_unused_gas_weight(1)
                .with_static_gas(ON_UNSTAKE_GAS)
                .on_unstake(lst_amount, unstake_amount, msg_hash, unstake_trigger),
        )
    }
}
