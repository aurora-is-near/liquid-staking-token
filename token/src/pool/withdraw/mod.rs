use near_plugins::{Pausable, pause};
use near_sdk::{CryptoHash, Gas, NearToken, Promise, env, near, require};

use crate::pool::unstake::{UnstakeMessage, WithdrawTokens};
use crate::{LiquidStakingToken, LiquidStakingTokenExt};

mod native;
pub mod requests;
mod wnear;

const ON_WITHDRAW_WNEAR_GAS: Gas = Gas::from_tgas(5);
const ON_WITHDRAW_NATIVE_GAS: Gas = Gas::from_tgas(3);
const REMOVE_LOCK_GAS: Gas = Gas::from_tgas(1);

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
    /// tranches stay queued for a future call. The tail of the FT chain
    /// unlocks the in-flight tranche so a failed delivery can be retried
    /// without resetting the cooldown. Partial wNEAR delivery leaves a
    /// residual tranche behind that the next `withdraw` call picks up.
    ///
    /// # Panics
    ///
    /// * `"Failed to hash the message"` — `args` cannot be borsh-serialized
    ///   (effectively unreachable for well-formed input).
    /// * `"There are no available tokens for withdrawal for this message
    ///   hash"` — the queue has no entry, no tranche is matured, or every
    ///   matured tranche is already locked by an in-flight withdrawal.
    /// * `"Unstake request is already in progress"` — a prior withdraw
    ///   for this same hash is still settling. Wait for the chain's tail
    ///   `remove_lock` callback to fire, then retry.
    #[pause]
    pub fn withdraw(&mut self, args: UnstakeMessage) -> Promise {
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

    /// Releases the in-flight lock on `hash` after a withdrawal attempt
    /// settles. Runs unconditionally as the tail of every `withdraw_*` chain
    /// so a failed FT call can be retried; no-op when the in-flight tranche
    /// was already removed by a successful full withdrawal.
    #[private]
    pub fn remove_lock(&mut self, hash: CryptoHash) {
        self.withdrawal_requests.release_lock(&hash);
    }
}
