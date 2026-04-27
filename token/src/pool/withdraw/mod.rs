use near_plugins::{Pausable, pause};
use near_sdk::{CryptoHash, Gas, NearToken, Promise, env, near, require};

use crate::pool::unstake::{UnstakeMessage, WithdrawTokens};
use crate::{LiquidStakingToken, LiquidStakingTokenExt};

mod native;
mod wnear;

const UNSTAKE_COOLDOWN_PERIOD: u64 = 4;
const ON_WITHDRAW_WNEAR_GAS: Gas = Gas::from_tgas(5);
const ON_WITHDRAW_NATIVE_GAS: Gas = Gas::from_tgas(3);
const REMOVE_LOCK_GAS: Gas = Gas::from_tgas(1);

#[derive(Debug, Default, Clone, Copy)]
#[near(serializers = [borsh])]
pub struct UserDistribution {
    /// The total NEAR-equivalent amount the user can claim from this entry.
    /// `withdrawal_amount - wnear_residual` is still held in NEAR form on the
    /// contract account; `wnear_residual` is already in wNEAR (refunded back
    /// from a prior partial `ft_transfer_call`).
    pub withdrawal_amount: NearToken,
    /// Epoch at which the user initiated the most recent unstake into this
    /// entry. Each subsequent unstake into the same entry resets this and
    /// therefore the cooldown.
    pub unstake_epoch: u64,
    /// How much of `withdrawal_amount` is already held as wNEAR by the
    /// contract account. On the next withdrawal, only the difference
    /// (`withdrawal_amount - wnear_residual`) needs a fresh `near_deposit`.
    /// Invariant: `wnear_residual <= withdrawal_amount`.
    pub wnear_residual: NearToken,
    /// Set after a previous attempt successfully paid `storage_deposit` to
    /// register the receiver on the wNEAR contract. Subsequent retries skip
    /// the storage_deposit step.
    pub storage_was_paid: bool,
}

#[near]
impl LiquidStakingToken {
    #[pause]
    pub fn withdraw(&mut self, args: UnstakeMessage) -> Promise {
        let msg_hash = args
            .hash()
            .unwrap_or_else(|_| env::panic_str("Failed to hash the message"));
        let (amount, epoch) = self.unstake_queue.get(&msg_hash).map_or_else(
            || env::panic_str("Account is not found in the unstake queue"),
            |entry| (&entry.withdrawal_amount, &entry.unstake_epoch),
        );

        require!(
            *epoch + UNSTAKE_COOLDOWN_PERIOD <= env::epoch_height(),
            "The cooldown hasn't passed yet"
        );

        match args.withdraw_tokens {
            WithdrawTokens::Native => self.withdraw_native(args.receiver_id, *amount, msg_hash),
            WithdrawTokens::Wnear { .. } => self.withdraw_wnear(*amount, args, msg_hash),
        }
    }

    #[private]
    pub fn remove_lock(&mut self, msg_hash: CryptoHash) {
        self.withdrawal_locks.remove(&msg_hash);
    }
}
