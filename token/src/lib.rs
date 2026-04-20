use near_contract_standards::fungible_token::FungibleToken;
use near_contract_standards::fungible_token::metadata::FungibleTokenMetadata;
use near_plugins::{AccessControlRole, AccessControllable, Pausable, Upgradable, access_control};
use near_sdk::borsh::BorshDeserialize;
use near_sdk::borsh::BorshSerialize;
use near_sdk::store::LookupMap;
use near_sdk::{
    AccountId, BorshStorageKey, CryptoHash, NearToken, PanicOnDefault, PublicKey, env, near,
    require,
};

mod core;
mod metadata;
pub mod pool;
mod receiver;
mod resolver;
mod storage;
mod traits;

pub const ONE_YOCTO: NearToken = NearToken::from_yoctonear(1);

#[derive(BorshSerialize, BorshStorageKey)]
#[borsh(crate = "near_sdk::borsh")]
enum StorageKey {
    FungibleToken,
    UnstakeQueue,
}

#[derive(AccessControlRole, Clone, Copy)]
#[near(serializers = [json])]
enum Role {
    Admin,
    SignatureVerifier,
    PauseManager,
    UnpauseManager,
}

#[derive(PanicOnDefault, Pausable, Upgradable)]
#[access_control(role_type(Role))]
#[upgradable(access_control_roles(
    code_stagers(Role::Admin),
    code_deployers(Role::Admin),
    duration_initializers(Role::Admin),
    duration_update_stagers(Role::Admin),
    duration_update_appliers(Role::Admin),
))]
#[pausable(
    pause_roles(Role::Admin, Role::PauseManager),
    unpause_roles(Role::Admin, Role::UnpauseManager)
)]
#[near(contract_state)]
pub struct LiquidStakingToken {
    token: FungibleToken,
    metadata: FungibleTokenMetadata,
    unstake_queue: LookupMap<CryptoHash, (u128, u64)>,
    owner_id: AccountId,
    wnear_id: AccountId,
    validator_public_key: PublicKey,
    /// Total NEAR backing the outstanding LST supply. Grows with staking
    /// rewards (reward-bearing model), so `total_staked_amount / ft_total_supply`
    /// is the current LST ↔ NEAR exchange rate.
    total_staked_amount: NearToken,
    /// Sum of NEAR amounts sitting in `unstake_queue`. Tracked explicitly so we
    /// can derive rewards from the account's locked balance without iterating
    /// the queue.
    total_pending_unstake: NearToken,
    /// Withdrawal fee in basis points (1 bp = 0.01%). Applied to the NEAR
    /// amount released on `withdraw`.
    withdrawal_fee_bps: u16,
    /// NEAR accumulated from withdrawal fees, claimable by `Role::Admin`.
    withdrawal_fees_collected: NearToken,
}

/// Maximum allowed withdrawal fee (10%). Hard-coded cap so the admin can't
/// strand user funds behind a runaway fee.
pub const MAX_WITHDRAWAL_FEE_BPS: u16 = 1_000;
/// Denominator for basis-point math.
pub const BPS_DENOMINATOR: u16 = 10_000;

#[near]
impl LiquidStakingToken {
    #[init]
    #[must_use]
    #[allow(clippy::use_self)]
    pub fn new(
        owner_id: AccountId,
        wnear_id: AccountId,
        validator_public_key: PublicKey,
        metadata: FungibleTokenMetadata,
        init_lock: Option<NearToken>, // The parameter mostly is used for tests since single node couldn't have 0 locked balances.
        withdrawal_fee_bps: Option<u16>,
    ) -> Self {
        require!(!env::state_exists(), "Already initialized");
        metadata.assert_valid();

        let withdrawal_fee_bps = withdrawal_fee_bps.unwrap_or(0);
        require!(
            withdrawal_fee_bps <= MAX_WITHDRAWAL_FEE_BPS,
            "withdrawal_fee_bps exceeds MAX_WITHDRAWAL_FEE_BPS"
        );

        let mut token = FungibleToken::new(StorageKey::FungibleToken);
        token.internal_register_account(&env::current_account_id());

        let mut contract = Self {
            token,
            metadata,
            unstake_queue: LookupMap::new(StorageKey::UnstakeQueue),
            owner_id: owner_id.clone(),
            wnear_id,
            validator_public_key,
            total_staked_amount: init_lock.unwrap_or(NearToken::ZERO),
            total_pending_unstake: NearToken::ZERO,
            withdrawal_fee_bps,
            withdrawal_fees_collected: NearToken::ZERO,
        };

        contract.grant_roles(&owner_id);
        contract
    }
}

impl LiquidStakingToken {
    fn grant_roles(&mut self, admin_account_id: &AccountId) {
        let mut acl = self.acl_get_or_init();
        acl.add_super_admin_unchecked(admin_account_id);

        acl.add_admin_unchecked(Role::Admin, admin_account_id);
        acl.add_admin_unchecked(Role::PauseManager, admin_account_id);
        acl.add_admin_unchecked(Role::UnpauseManager, admin_account_id);

        acl.grant_role_unchecked(Role::Admin, admin_account_id);

        acl.grant_role_unchecked(Role::PauseManager, admin_account_id);
        acl.grant_role_unchecked(Role::UnpauseManager, admin_account_id);
    }
}
