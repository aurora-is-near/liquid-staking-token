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
    #[pause]
    pub fn withdraw(&mut self, args: UnstakeMessage) -> Promise {
        let msg_hash = args
            .hash()
            .unwrap_or_else(|_| env::panic_str("Failed to hash the message"));

        let current_epoch = env::epoch_height();
        let amount = self
            .withdrawal_requests
            .amount_of_matured_tranches(current_epoch, msg_hash);

        require!(
            amount > NearToken::ZERO,
            "There are no available tokens for withdrawal for this message hash"
        );

        match args.withdraw_tokens {
            WithdrawTokens::Native => Self::withdraw_native(args.receiver_id, amount, msg_hash),
            WithdrawTokens::Wnear { .. } => self.withdraw_wnear(amount, args, msg_hash),
        }
    }

    /// Releases the in-flight lock on `msg_hash` after a withdrawal attempt
    /// settles. Runs unconditionally as the tail of every `withdraw_*` chain
    /// so a failed FT call can be retried; no-op when the in-flight tranche
    /// was already removed by a successful full withdrawal.
    #[private]
    pub fn remove_lock(&mut self, msg_hash: CryptoHash) {
        self.withdrawal_requests.release_lock(&msg_hash);
    }
}
