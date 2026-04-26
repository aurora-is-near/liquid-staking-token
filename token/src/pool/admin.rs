use near_plugins::{AccessControllable, access_control_any};
use near_sdk::{Promise, PublicKey, assert_one_yocto, env, near, require};

use crate::{LiquidStakingToken, LiquidStakingTokenExt, Role};

const MAX_PROTOCOL_FEE_BPS: u128 = 2_000; // 20%

#[near]
impl LiquidStakingToken {
    /// Updates the protocol fee. Capped at [`MAX_PROTOCOL_FEE_BPS`].
    #[access_control_any(roles(Role::Admin))]
    pub fn set_protocol_fee_bps(&mut self, fee_bps: u16) {
        require!(
            u128::from(fee_bps) <= MAX_PROTOCOL_FEE_BPS,
            "protocol fee exceeds MAX_PROTOCOL_FEE_BPS, which is 20%"
        );

        self.statistics.protocol_fee_bps = fee_bps;
    }

    /// Adds a new full access key to the contract.
    #[payable]
    #[access_control_any(roles(Role::Admin))]
    pub fn add_full_access_key(&mut self, public_key: PublicKey) -> Promise {
        assert_one_yocto();
        Promise::new(env::current_account_id()).add_full_access_key(public_key)
    }
}
