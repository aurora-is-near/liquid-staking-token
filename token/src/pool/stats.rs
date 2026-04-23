use near_sdk::{NearToken, env, near};

pub const BPS_DENOMINATOR: u16 = 10_000;

/// Statistics about the pool
#[derive(Debug, Default, Clone, Copy)]
#[near(serializers = [borsh])]
pub struct PoolStatistics {
    /// Total amount of staked NEAR tokens
    pub total_staked_amount: NearToken,
    /// The sum of NEAR amounts queued for withdrawal
    pub total_pending_unstake: NearToken,
    /// The current withdrawal fee in basis points (1 bp = 0.01%)
    pub withdrawal_fee_bps: u16,
    /// The total amount of fees collected during withdrawals
    pub withdrawal_collected_fees: NearToken,
    /// The number of epochs since the last rewards sync.
    pub last_epoch_synced: u64,
    /// The number of delegators in the pool.
    pub num_delegators: u64,
}

impl PoolStatistics {
    #[must_use]
    pub fn with_init_lock(total_staked_amount: NearToken) -> Self {
        Self {
            total_staked_amount,
            ..Default::default()
        }
    }

    pub fn increase_delegators(&mut self) {
        self.num_delegators = self
            .num_delegators
            .checked_add(1)
            .unwrap_or_else(|| env::panic_str("Overflow while increasing delegator"));
    }

    pub fn decrease_delegators(&mut self) {
        self.num_delegators = self
            .num_delegators
            .checked_sub(1)
            .unwrap_or_else(|| env::panic_str("Underflow while decreasing delegator"));
    }
}
