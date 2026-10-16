use near_plugins::{Pausable, pause};
use near_sdk::{Gas, NearToken, Promise, env, near, require};

use crate::pool::unstake::{UnstakeMessage, WithdrawTokens};
use crate::pool::{GAS_RESERVE, STORAGE_DEPOSIT_GAS, assert_gas, calculate_min_gas};
use crate::traits::NEAR_DEPOSIT_GAS;
use crate::{LiquidStakingToken, LiquidStakingTokenExt};

mod native;
pub mod requests;
mod wnear;

const ON_WITHDRAW_WNEAR_GAS: Gas = Gas::from_tgas(15);
const ON_WITHDRAW_NATIVE_GAS: Gas = Gas::from_tgas(15);

#[near]
impl LiquidStakingToken {
    /// Claims every matured tranche queued under `args` and dispatches the
    /// delivery (native NEAR `transfer` or wNEAR FT chain) to the
    /// `receiver_id` recorded on the original [`UnstakeMessage`].
    ///
    /// Convenience wrapper over [`Self::withdraw_by_hash`] that hashes
    /// `args` for the caller. Use this when the caller still has the
    /// original `UnstakeMessage` JSON; if you only know the hash, call
    /// `withdraw_by_hash` directly to skip the redundant hashing.
    ///
    /// # Behavior
    ///
    /// On the queue side, every currently-matured tranche under the hash
    /// is collapsed into a single in-flight (locked) tranche; immature
    /// tranches stay queued for a future call. The settlement callback
    /// updates accounting and unlocks the in-flight tranche atomically, so a
    /// known failed delivery can be retried without resetting the cooldown.
    /// Partial wNEAR delivery leaves a residual tranche behind that the next
    /// `withdraw` call picks up. Failed settlement keeps the claim locked.
    ///
    /// # Panics
    ///
    /// * `"Failed to hash the message"` — `args` cannot be borsh-serialized
    ///   (effectively unreachable for well-formed input).
    /// * `"There are no available tokens for withdrawal for this message
    ///   hash"` — the queue has no entry, no tranche is matured, or every
    ///   matured tranche is already locked by an in-flight withdrawal.
    /// * `"Unstake request is already in progress"` — a prior withdraw
    ///   for this same hash is still settling. Wait for settlement to complete
    ///   before retrying. Failed settlement requires admin reconciliation.
    #[pause]
    pub fn withdraw(&mut self, args: UnstakeMessage) -> Promise {
        // Check the delivery and settlement budget before locking or sending anything.
        assert_gas(withdraw_gas(&args));
        let current_epoch = env::epoch_height();
        let hash = args
            .hash()
            .unwrap_or_else(|_| env::panic_str("Failed to hash the message"));
        let amount = self
            .withdrawal_requests
            .amount_of_matured_tranches(current_epoch, hash);

        require!(
            amount > NearToken::ZERO,
            "There are no available tokens for withdrawal for this message hash"
        );

        match args.withdraw_tokens {
            WithdrawTokens::Native => Self::withdraw_native(args.receiver_id, amount, hash),
            WithdrawTokens::Wnear { .. } => self.withdraw_wnear(args, amount, hash),
        }
    }
}

fn withdraw_gas(args: &UnstakeMessage) -> Gas {
    let delivery_gas = match &args.withdraw_tokens {
        WithdrawTokens::Native => ON_WITHDRAW_NATIVE_GAS,
        WithdrawTokens::Wnear {
            is_storage_deposit,
            msg,
            min_gas,
            ..
        } => {
            let mut gas = NEAR_DEPOSIT_GAS.saturating_add(ON_WITHDRAW_WNEAR_GAS);

            if args.receiver_id != env::current_account_id() {
                gas = gas.saturating_add(calculate_min_gas(*min_gas, msg.is_some()));

                if *is_storage_deposit {
                    gas = gas.saturating_add(STORAGE_DEPOSIT_GAS);
                }
            }

            gas
        }
    };

    GAS_RESERVE.saturating_add(delivery_gas)
}
