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

use crate::pool::{PoolStatistics, UserDistribution};

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
    /// The underlying fungible token represented LST token.
    token: FungibleToken,
    /// The metadata of the LST token.
    metadata: FungibleTokenMetadata,
    /// The queue for unstake requests.
    unstake_queue: LookupMap<CryptoHash, UserDistribution>,
    /// The ID of the account that owns the contract.
    owner_id: AccountId,
    /// The ID of the account that holds wNEAR.
    wnear_id: AccountId,
    /// The public key of the validator.
    validator_public_key: PublicKey,
    /// The pool statistics.
    statistics: PoolStatistics,
}

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
        init_lock: Option<NearToken>,
    ) -> Self {
        require!(!env::state_exists(), "Already initialized");
        metadata.assert_valid();

        let mut token = FungibleToken::new(StorageKey::FungibleToken);
        token.internal_register_account(&env::current_account_id());

        let mut contract = Self {
            token,
            metadata,
            unstake_queue: LookupMap::new(StorageKey::UnstakeQueue),
            owner_id: owner_id.clone(),
            wnear_id,
            validator_public_key,
            statistics: init_lock
                .map_or_else(PoolStatistics::default, PoolStatistics::with_init_lock),
        };

        if let Some(init_lock) = init_lock {
            contract.mint_shared_to_owner(&owner_id, init_lock);
        }

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

    fn mint_shared_to_owner(&mut self, owner_id: &AccountId, init_lock: NearToken) {
        let lst = &mut self.token;

        if owner_id != &env::current_account_id() {
            lst.internal_register_account(owner_id);
        }

        lst.internal_deposit(owner_id, init_lock.as_yoctonear());
        self.statistics.increase_delegators();
    }
}
