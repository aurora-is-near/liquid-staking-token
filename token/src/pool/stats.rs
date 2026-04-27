use near_sdk::{NearToken, env, near};

pub const BPS_DENOMINATOR: u16 = 10_000;

/// Statistics about the pool
#[derive(Debug, Default, Clone, Copy)]
#[near(serializers = [borsh])]
pub struct PoolStatistics {
    /// Total balance (locked + unlocked) of the pool in NEAR tokens.
    pub latest_total_balance: NearToken,
    /// Total amount of staked NEAR tokens
    pub total_staked_amount: NearToken,
    /// The sum of NEAR amounts queued for withdrawal
    pub total_pending_withdrawals: NearToken,
    /// The current withdrawal fee in basis points (1 bp = 0.01%)
    pub protocol_fee_bps: u16,
    /// The number of epochs since the last rewards sync.
    pub last_epoch_synced: u64,
    /// The number of delegators in the pool.
    pub num_delegators: u64,
}

impl PoolStatistics {
    pub fn increase_stake_amount(&mut self, amount: NearToken) {
        self.total_staked_amount = self
            .total_staked_amount
            .checked_add(amount)
            .unwrap_or_else(|| env::panic_str("Overflow while adding stake amount"));
    }

    pub fn decrease_stake_amount(&mut self, amount: NearToken) {
        self.total_staked_amount = self
            .total_staked_amount
            .checked_sub(amount)
            .unwrap_or_else(|| env::panic_str("Underflow while removing stake amount"));
    }

    pub fn increase_total_balance(&mut self, amount: NearToken) {
        self.latest_total_balance = self
            .latest_total_balance
            .checked_add(amount)
            .unwrap_or_else(|| env::panic_str("Overflow while adding total balance"));
    }

    pub fn decrease_total_balance(&mut self, amount: NearToken) {
        self.latest_total_balance = self
            .latest_total_balance
            .checked_sub(amount)
            .unwrap_or_else(|| env::panic_str("Underflow while removing total balance"));
    }

    pub fn increase_pending_withdrawals(&mut self, amount: NearToken) {
        self.total_pending_withdrawals = self
            .total_pending_withdrawals
            .checked_add(amount)
            .unwrap_or_else(|| env::panic_str("Overflow while adding pending withdrawals"));
    }

    pub fn decrease_pending_withdrawals(&mut self, amount: NearToken) {
        self.total_pending_withdrawals = self
            .total_pending_withdrawals
            .checked_sub(amount)
            .unwrap_or_else(|| env::panic_str("Underflow while removing pending withdrawals"));
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
