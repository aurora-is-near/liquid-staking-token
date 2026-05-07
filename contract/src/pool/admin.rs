use near_plugins::{AccessControllable, access_control_any};
use near_sdk::{CryptoHash, Promise, PublicKey, assert_one_yocto, env, near, require};

use crate::pool::RESTAKE_GAS;
use crate::{LiquidStakingToken, LiquidStakingTokenExt, Role};

const MAX_PROTOCOL_FEE_BPS: u16 = 2_000; // 20%

#[near]
impl LiquidStakingToken {
    /// Updates the protocol fee. Capped at [`MAX_PROTOCOL_FEE_BPS`].
    #[payable]
    #[access_control_any(roles(Role::Admin))]
    pub fn set_protocol_fee_bps(&mut self, fee_bps: u16) {
        assert_one_yocto();
        require!(
            fee_bps <= MAX_PROTOCOL_FEE_BPS,
            "protocol fee exceeds MAX_PROTOCOL_FEE_BPS, which is 20%"
        );

        self.sync_rewards_internal(None);
        self.statistics.protocol_fee_bps = fee_bps;
    }

    /// Updates the validator public key.
    #[payable]
    #[access_control_any(roles(Role::Admin))]
    pub fn set_validator_public_key(&mut self, validator_public_key: PublicKey) -> Promise {
        assert_one_yocto();
        near_sdk::log!("Validator public key set to: {validator_public_key}");
        self.validator_public_key = validator_public_key;

        self.restake_promise().then(
            Self::ext(env::current_account_id())
                .with_unused_gas_weight(1)
                .with_static_gas(RESTAKE_GAS)
                .on_restake(),
        )
    }

    /// Adds a new full access key to the contract.
    #[payable]
    #[access_control_any(roles(Role::Admin))]
    pub fn add_full_access_key(&mut self, public_key: PublicKey) -> Promise {
        assert_one_yocto();
        Promise::new(env::current_account_id()).add_full_access_key(public_key)
    }

    /// Releases the lock for a withdrawal request identified by `hash`.
    #[payable]
    #[access_control_any(roles(Role::Admin))]
    pub fn force_release_lock(&mut self, hash: CryptoHash) -> bool {
        assert_one_yocto();
        if self.withdrawal_requests.release_lock(&hash) {
            near_sdk::log!("The lock for the withdrawal request has been released");
            true
        } else {
            near_sdk::log!("No lock found for withdrawal request");
            false
        }
    }
}
