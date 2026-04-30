use defuse_near_utils::Lock;
use near_sdk::store::IterableMap;
use near_sdk::{CryptoHash, IntoStorageKey, NearToken, env, near, require};

const UNSTAKE_COOLDOWN_PERIOD: u64 = 4;

/// Pending withdrawal queue, keyed by the hash of the originating
/// [`UnstakeMessage`](crate::pool::UnstakeMessage).
///
/// Each `msg_hash` maps to a list of independent [`Tranche`]s. Every successful
/// `on_unstake` appends one — folding into a same-epoch sibling and collapsing
/// already-matured tranches to keep the vec small — and `withdraw` collapses
/// every currently-matured tranche under the hash into a single in-flight
/// delivery.
///
/// # Invariants
///
/// * **At most one locked tranche per `msg_hash`.** It represents the
///   in-flight withdrawal whose FT chain is still settling. While it exists,
///   [`Self::amount_of_matured_tranches`] rejects further sweeps with
///   `"Unstake request is already in progress"`.
/// * **Locked tranches are never mutated by [`Self::append_request`].** A new
///   unstake that arrives mid-withdrawal is appended as a separate unlocked
///   tranche; it is never folded into the in-flight one and never disturbs
///   it.
/// * **Cooldown independence.** A tranche's `unstake_epoch` is set once at
///   creation and never shifted by a later unstake — each tranche matures
///   on its own schedule.
/// * **Bounded vec length.** Same-epoch unstakes fold together and matured
///   tranches collapse on the next `append_request`, so the vec is bounded
///   by roughly `UNSTAKE_COOLDOWN_PERIOD + 2` regardless of unstake
///   frequency on a hash.
///
/// # Fields
///
/// * `requests` — backing map keyed by [`CryptoHash`] (the `UnstakeMessage`
///   hash). Stored as an [`IterableMap`] so future admin tooling can
///   list outstanding requests; the contract itself only ever does
///   point lookups.
#[derive(Debug)]
#[near(serializers = [borsh])]
pub struct WithdrawalRequests {
    requests: IterableMap<CryptoHash, Vec<Lock<Tranche>>>,
}

impl WithdrawalRequests {
    pub(crate) fn new<S: IntoStorageKey>(prefix: S) -> Self {
        Self {
            requests: IterableMap::new(prefix),
        }
    }

    /// Records a successful unstake under `msg_hash`. Bounds the queue's vec
    /// length, so the contract is resistant to bloat from repeated tiny
    /// unstakes:
    ///   * matured, **unlocked** tranches collapse into a single tranche —
    ///     their cooldowns are already past, so the merge is lossless;
    ///   * the new amount folds into an existing unlocked tranche at the
    ///     same `current_epoch` if one is present — same-epoch tranches
    ///     share a cooldown, so this is lossless too.
    ///     Locked (in-flight) tranches are never touched: a withdrawal mid-flight
    ///     keeps its single locked tranche; `append_request` neither collapses
    ///     it nor folds amounts into it.
    pub(crate) fn append_request(
        &mut self,
        current_epoch: u64,
        msg_hash: CryptoHash,
        amount: NearToken,
    ) {
        let tranches = self.requests.entry(msg_hash).or_default();

        if tranches.is_empty() {
            tranches.push(Lock::unlocked(Tranche::new(amount, current_epoch)));
            return;
        }

        let collapsed = Self::collapse_matured_tranches(tranches, current_epoch);

        if let Some(tranche) = collapsed {
            tranches.push(Lock::unlocked(tranche));
        }

        if let Some(existing) = tranches
            .iter_mut()
            .find(|t| t.get().is_some_and(|t| t.unstake_epoch == current_epoch))
        {
            let tranche = existing.as_inner_unchecked_mut();

            tranche.withdrawal_amount = tranche
                .withdrawal_amount
                .checked_add(amount)
                .unwrap_or_else(|| env::panic_str("Overflow while merging same-epoch unstake"));
        } else {
            tranches.push(Lock::unlocked(Tranche::new(amount, current_epoch)));
        }
    }

    /// Sweeps all matured, unlocked tranches under `msg_hash` and merges them
    /// into a single in-flight (locked) tranche, leaving any immature tranches
    /// untouched. Returns the total claimable amount carried by the in-flight
    /// tranche. Panics if a withdrawal is already in flight (any tranche already locked).
    pub(crate) fn amount_of_matured_tranches(
        &mut self,
        current_epoch: u64,
        msg_hash: CryptoHash,
    ) -> NearToken {
        let Some(tranches) = self.requests.get_mut(&msg_hash) else {
            return NearToken::ZERO;
        };

        require!(
            tranches.iter().all(|t| !t.is_locked()),
            "Unstake request is already in progress"
        );

        let collapsed = Self::collapse_matured_tranches(tranches, current_epoch);

        collapsed.map_or(NearToken::ZERO, |tranche| {
            let amount = tranche.withdrawal_amount;
            tranches.push(Lock::locked(tranche));

            amount
        })
    }

    fn collapse_matured_tranches(
        tranches: &mut Vec<Lock<Tranche>>,
        current_epoch: u64,
    ) -> Option<Tranche> {
        let mut matured_amount = NearToken::ZERO;
        let mut matured_residual = NearToken::ZERO;
        let mut matured_storage = false;
        let mut matured_epoch = 0;
        let mut i = 0;

        while i < tranches.len() {
            // Only collect unlocked, matured tranches. Locked (in-flight)
            // tranches are skipped so a concurrent withdrawal isn't disturbed,
            // and unlocked-but-still-in-cooldown tranches stay queued for a
            // later sweep.
            let is_unlocked_and_matured = tranches[i]
                .get()
                .is_some_and(|t| t.is_matured(current_epoch));

            if !is_unlocked_and_matured {
                i += 1;
                continue;
            }

            let tranche = tranches[i].as_inner_unchecked();
            matured_amount = matured_amount
                .checked_add(tranche.withdrawal_amount)
                .unwrap_or_else(|| env::panic_str("Overflow while collapsing withdrawal amount"));
            matured_residual = matured_residual
                .checked_add(tranche.wnear_residual)
                .unwrap_or_else(|| env::panic_str("Overflow while collapsing residuals"));
            matured_storage |= tranche.storage_was_paid;

            if tranche.unstake_epoch > matured_epoch {
                matured_epoch = tranche.unstake_epoch;
            }

            // `swap_remove` puts the last element at index `i`; we must
            // re-test it on the next iteration without advancing `i`.
            tranches.swap_remove(i);
        }

        (matured_amount > NearToken::ZERO).then_some(Tranche {
            withdrawal_amount: matured_amount,
            unstake_epoch: matured_epoch,
            wnear_residual: matured_residual,
            storage_was_paid: matured_storage,
        })
    }

    /// Returns the in-flight (locked) tranche under `msg_hash`. There is at
    /// most one per `msg_hash` because `collapse_matured_tranches` rejects
    /// new sweeps while a prior tranche is still locked. Panics if missing.
    pub(super) fn locked_tranche(&self, msg_hash: &CryptoHash) -> &Tranche {
        self.requests
            .get(msg_hash)
            .unwrap_or_else(|| env::panic_str("No withdrawal for the given hash"))
            .iter()
            .find_map(Lock::as_locked)
            .unwrap_or_else(|| env::panic_str("The user withdrawal should be locked at this point"))
    }

    pub(super) fn locked_tranche_mut(&mut self, msg_hash: &CryptoHash) -> &mut Tranche {
        self.requests
            .get_mut(msg_hash)
            .unwrap_or_else(|| env::panic_str("No withdrawal for the given hash"))
            .iter_mut()
            .find_map(Lock::as_locked_mut)
            .unwrap_or_else(|| env::panic_str("The user withdrawal should be locked at this point"))
    }

    /// Drops the in-flight (locked) tranche from `msg_hash`. Any unlocked
    /// (queued, possibly non-matured) tranches under the same hash are
    /// preserved. If no tranches remain, the queue entry is removed entirely.
    /// No-op if no tranche is locked.
    pub fn remove_request(&mut self, msg_hash: &CryptoHash) {
        let Some(tranches) = self.requests.get_mut(msg_hash) else {
            return;
        };

        tranches.retain(|tranche| !tranche.is_locked());

        if tranches.is_empty() {
            self.requests.remove(msg_hash);
        }
    }

    /// Unlocks the in-flight tranche under `msg_hash` so a future `withdraw`
    /// can retry. No-op if no tranche is locked (e.g. the in-flight tranche
    /// was already removed by a successful full withdrawal).
    pub fn release_lock(&mut self, msg_hash: &CryptoHash) {
        let Some(tranches) = self.requests.get_mut(msg_hash) else {
            return;
        };

        if let Some(tranche) = tranches.iter_mut().find(|t| t.is_locked()) {
            tranche.force_unlock();
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
#[near(serializers = [borsh])]
pub struct Tranche {
    /// The total NEAR-equivalent amount the user can claim from this entry.
    /// `withdrawal_amount - wnear_residual` is still held in NEAR form on the
    /// contract account; `wnear_residual` is already in wNEAR (refunded back
    /// from a prior partial `ft_transfer_call`).
    pub withdrawal_amount: NearToken,
    /// Epoch at which the unstake that produced this tranche was executed.
    /// Each unstake appends an independent tranche; this field is set once at
    /// creation and never shifted by later unstakes. When matured tranches
    /// are merged into an in-flight tranche at `withdraw` time, this field
    /// is set to the max epoch of the merged tranches — which is still
    /// matured, so the residual remains immediately reclaimable.
    unstake_epoch: u64,
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

impl Tranche {
    pub fn new(withdrawal_amount: NearToken, unstake_epoch: u64) -> Self {
        Self {
            withdrawal_amount,
            unstake_epoch,
            ..Default::default()
        }
    }

    #[inline]
    const fn is_matured(&self, current_epoch: u64) -> bool {
        self.unstake_epoch + UNSTAKE_COOLDOWN_PERIOD <= current_epoch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::borsh::BorshSerialize;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::{BorshStorageKey, testing_env};

    #[derive(BorshSerialize, BorshStorageKey)]
    #[borsh(crate = "near_sdk::borsh")]
    enum StorageKey {
        Requests,
    }

    fn setup() -> WithdrawalRequests {
        testing_env!(VMContextBuilder::new().build());
        WithdrawalRequests::new(StorageKey::Requests)
    }

    fn hash(seed: u8) -> CryptoHash {
        let mut h = [0u8; 32];
        h[0] = seed;
        h
    }

    fn near(yocto: u128) -> NearToken {
        NearToken::from_yoctonear(yocto)
    }

    /// Direct vec accessor for assertions (bypasses the `locked_tranche*`
    /// public surface since several tests need to inspect non-locked tranches).
    fn vec_at<'a>(wr: &'a WithdrawalRequests, h: &CryptoHash) -> Option<&'a Vec<Lock<Tranche>>> {
        wr.requests.get(h)
    }

    // -----------------------------------------------------------------
    // Tranche::is_matured boundary
    // -----------------------------------------------------------------

    #[test]
    fn tranche_matures_exactly_at_cooldown_boundary() {
        let t = Tranche {
            unstake_epoch: 10,
            ..Default::default()
        };
        assert!(!t.is_matured(10 + UNSTAKE_COOLDOWN_PERIOD - 1));
        assert!(t.is_matured(10 + UNSTAKE_COOLDOWN_PERIOD));
        assert!(t.is_matured(10 + UNSTAKE_COOLDOWN_PERIOD + 1));
    }

    // -----------------------------------------------------------------
    // append_request — first call creates the entry
    // -----------------------------------------------------------------

    #[test]
    fn append_request_creates_entry_on_first_call() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(10, h, near(100));

        let v = vec_at(&wr, &h).expect("entry should be created");
        assert_eq!(v.len(), 1);
        let t = v[0].as_inner_unchecked();
        assert_eq!(t.withdrawal_amount, near(100));
        assert_eq!(t.unstake_epoch, 10);
        assert!(!v[0].is_locked());
    }

    // -----------------------------------------------------------------
    // append_request — same-epoch fold
    // -----------------------------------------------------------------

    #[test]
    fn append_request_folds_same_epoch_into_one_tranche() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(10, h, near(100));
        wr.append_request(10, h, near(50));

        let v = vec_at(&wr, &h).expect("entry exists");
        assert_eq!(v.len(), 1, "same-epoch unstakes should fold");
        let t = v[0].as_inner_unchecked();
        assert_eq!(t.withdrawal_amount, near(150));
        assert_eq!(t.unstake_epoch, 10);
    }

    // -----------------------------------------------------------------
    // append_request — different epochs within cooldown stay independent
    // -----------------------------------------------------------------

    #[test]
    fn append_request_different_epochs_within_cooldown_stay_independent() {
        let mut wr = setup();
        let h = hash(1);

        // both at epochs 10 and 11 — neither is matured at the time of the
        // second append (current_epoch = 11, cooldown = 4, so matured at >=14)
        wr.append_request(10, h, near(100));
        wr.append_request(11, h, near(50));

        let v = vec_at(&wr, &h).expect("entry exists");
        assert_eq!(v.len(), 2, "different in-cooldown epochs stay separate");

        let mut amounts: Vec<u128> = v
            .iter()
            .map(|l| l.as_inner_unchecked().withdrawal_amount.as_yoctonear())
            .collect();
        amounts.sort_unstable();
        assert_eq!(amounts, vec![50, 100]);
    }

    // -----------------------------------------------------------------
    // append_request — matured prior tranche collapses into a single tranche
    // -----------------------------------------------------------------

    #[test]
    fn append_request_collapses_matured_prior_tranche() {
        let mut wr = setup();
        let h = hash(1);

        // T1 at epoch 10, then a fresh unstake at epoch 14 — T1 is now matured.
        wr.append_request(10, h, near(100));
        wr.append_request(14, h, near(70));

        let v = vec_at(&wr, &h).expect("entry exists");
        assert_eq!(
            v.len(),
            2,
            "one matured-collapsed + one current-epoch tranche"
        );

        let mut tranches: Vec<&Tranche> = v.iter().map(Lock::as_inner_unchecked).collect();
        tranches.sort_by_key(|t| t.unstake_epoch);

        // matured: epoch=10, amount=100
        assert_eq!(tranches[0].unstake_epoch, 10);
        assert_eq!(tranches[0].withdrawal_amount, near(100));

        // new: epoch=14, amount=70
        assert_eq!(tranches[1].unstake_epoch, 14);
        assert_eq!(tranches[1].withdrawal_amount, near(70));
    }

    #[test]
    fn append_request_collapses_multiple_matured_into_one() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, h, near(100));
        wr.append_request(6, h, near(50));
        // both above are matured at epoch 10 (cooldown = 4)
        wr.append_request(10, h, near(30));

        let v = vec_at(&wr, &h).expect("entry exists");
        assert_eq!(v.len(), 2, "two matured collapsed + one new");

        let mut tranches: Vec<&Tranche> = v.iter().map(Lock::as_inner_unchecked).collect();
        tranches.sort_by_key(|t| t.unstake_epoch);

        // collapsed-matured: max_epoch = 6, amount = 150
        assert_eq!(tranches[0].unstake_epoch, 6);
        assert_eq!(tranches[0].withdrawal_amount, near(150));

        // new: epoch = 10, amount = 30
        assert_eq!(tranches[1].unstake_epoch, 10);
        assert_eq!(tranches[1].withdrawal_amount, near(30));
    }

    // -----------------------------------------------------------------
    // append_request — locked (in-flight) tranches are never touched
    // -----------------------------------------------------------------

    #[test]
    fn append_request_skips_locked_tranche() {
        let mut wr = setup();
        let h = hash(1);

        // Stage: one unlocked-matured + one locked in-flight under the same hash.
        wr.append_request(5, h, near(100));
        let _ = wr.amount_of_matured_tranches(10, h); // locks the merged tranche

        // A new unstake while a withdrawal is mid-flight.
        wr.append_request(10, h, near(40));

        let v = vec_at(&wr, &h).expect("entry exists");

        let locked_count = v.iter().filter(|l| l.is_locked()).count();
        assert_eq!(locked_count, 1, "in-flight lock should be preserved");

        let locked_amount = v
            .iter()
            .find(|l| l.is_locked())
            .unwrap()
            .as_inner_unchecked()
            .withdrawal_amount;
        assert_eq!(locked_amount, near(100), "locked tranche unchanged");

        let unlocked: Vec<&Tranche> = v
            .iter()
            .filter(|l| !l.is_locked())
            .map(Lock::as_inner_unchecked)
            .collect();
        assert_eq!(unlocked.len(), 1);
        assert_eq!(unlocked[0].withdrawal_amount, near(40));
        assert_eq!(unlocked[0].unstake_epoch, 10);
    }

    // -----------------------------------------------------------------
    // amount_of_matured_tranches
    // -----------------------------------------------------------------

    #[test]
    fn amount_of_matured_tranches_returns_zero_for_missing_hash() {
        let mut wr = setup();
        assert_eq!(
            wr.amount_of_matured_tranches(100, hash(99)),
            NearToken::ZERO
        );
    }

    #[test]
    fn amount_of_matured_tranches_returns_zero_when_nothing_matured() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(10, h, near(100));
        // current_epoch=11: T1 not yet matured.
        assert_eq!(wr.amount_of_matured_tranches(11, h), NearToken::ZERO);

        // The non-matured tranche must remain unlocked (no spurious lock).
        let v = vec_at(&wr, &h).unwrap();
        assert_eq!(v.len(), 1);
        assert!(!v[0].is_locked());
    }

    #[test]
    fn amount_of_matured_tranches_collapses_and_locks_result() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, h, near(100));
        wr.append_request(6, h, near(50));

        let amount = wr.amount_of_matured_tranches(10, h);
        assert_eq!(amount, near(150));

        let v = vec_at(&wr, &h).unwrap();
        let locked: Vec<&Lock<Tranche>> = v.iter().filter(|l| l.is_locked()).collect();
        assert_eq!(locked.len(), 1, "exactly one locked in-flight tranche");
        assert_eq!(locked[0].as_inner_unchecked().withdrawal_amount, near(150));
    }

    #[test]
    fn amount_of_matured_tranches_preserves_non_matured() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, h, near(100)); // matured at epoch 9
        wr.append_request(8, h, near(20)); // matured at epoch 12

        let amount = wr.amount_of_matured_tranches(10, h);
        assert_eq!(amount, near(100), "only T1 has matured");

        let v = vec_at(&wr, &h).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v.iter().filter(|l| l.is_locked()).count(), 1);

        // the surviving unlocked tranche is the non-matured one
        let unlocked = v
            .iter()
            .find(|l| !l.is_locked())
            .unwrap()
            .as_inner_unchecked();
        assert_eq!(unlocked.unstake_epoch, 8);
        assert_eq!(unlocked.withdrawal_amount, near(20));
    }

    #[test]
    #[should_panic(expected = "Unstake request is already in progress")]
    fn amount_of_matured_tranches_panics_when_already_in_flight() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, h, near(100));
        let _ = wr.amount_of_matured_tranches(10, h);

        // Second call while previous is still locked.
        let _ = wr.amount_of_matured_tranches(10, h);
    }

    #[test]
    fn amount_of_matured_tranches_residual_and_storage_aggregate() {
        let mut wr = setup();
        let h = hash(1);

        // Manually inject two matured tranches with residual / storage_was_paid
        // set, simulating the state after two prior partial-refund retries.
        wr.append_request(5, h, near(100));
        wr.append_request(6, h, near(50));

        {
            let v = wr.requests.get_mut(&h).unwrap();
            for lock in v.iter_mut() {
                let inner = lock.as_inner_unchecked_mut();
                inner.wnear_residual = near(7);
                inner.storage_was_paid = true;
            }
        }

        let _ = wr.amount_of_matured_tranches(10, h);

        let inflight = wr.locked_tranche(&h);
        assert_eq!(inflight.withdrawal_amount, near(150));
        assert_eq!(inflight.wnear_residual, near(14), "residuals are summed");
        assert!(inflight.storage_was_paid, "storage_was_paid is OR'd");
        assert_eq!(inflight.unstake_epoch, 6, "max of merged epochs");
    }

    // -----------------------------------------------------------------
    // locked_tranche / locked_tranche_mut
    // -----------------------------------------------------------------

    #[test]
    fn locked_tranche_returns_the_inflight_one() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, h, near(100));
        let _ = wr.amount_of_matured_tranches(10, h);

        let t = wr.locked_tranche(&h);
        assert_eq!(t.withdrawal_amount, near(100));
    }

    #[test]
    fn locked_tranche_finds_inflight_among_unlocked_tranches() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, h, near(100));
        let _ = wr.amount_of_matured_tranches(10, h);
        // Another unstake lands while withdraw is mid-flight.
        wr.append_request(10, h, near(33));

        let t = wr.locked_tranche(&h);
        assert_eq!(
            t.withdrawal_amount,
            near(100),
            "must return the locked tranche, not the new unlocked one"
        );

        let t_mut = wr.locked_tranche_mut(&h);
        assert_eq!(t_mut.withdrawal_amount, near(100));
    }

    #[test]
    #[should_panic(expected = "No withdrawal for the given hash")]
    fn locked_tranche_panics_on_missing_hash() {
        let wr = setup();
        let _ = wr.locked_tranche(&hash(99));
    }

    #[test]
    #[should_panic(expected = "should be locked at this point")]
    fn locked_tranche_panics_when_no_lock() {
        let mut wr = setup();
        let h = hash(1);
        wr.append_request(10, h, near(100));
        let _ = wr.locked_tranche(&h);
    }

    // -----------------------------------------------------------------
    // remove_request
    // -----------------------------------------------------------------

    #[test]
    fn remove_request_drops_only_locked_tranche() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, h, near(100));
        let _ = wr.amount_of_matured_tranches(10, h); // locks 100
        wr.append_request(10, h, near(40)); // unlocked tranche at epoch 10

        wr.remove_request(&h);

        let v = vec_at(&wr, &h).expect("entry survives because non-locked tranche remains");
        assert_eq!(v.len(), 1);
        assert!(!v[0].is_locked());
        assert_eq!(v[0].as_inner_unchecked().withdrawal_amount, near(40));
    }

    #[test]
    fn remove_request_removes_entry_when_vec_empties() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, h, near(100));
        let _ = wr.amount_of_matured_tranches(10, h);

        wr.remove_request(&h);

        assert!(
            vec_at(&wr, &h).is_none(),
            "fully drained entry should be removed"
        );
    }

    #[test]
    fn remove_request_no_op_when_nothing_locked() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(10, h, near(100));
        wr.remove_request(&h);

        let v = vec_at(&wr, &h).expect("entry untouched");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].as_inner_unchecked().withdrawal_amount, near(100));
    }

    #[test]
    fn remove_request_no_op_for_missing_hash() {
        let mut wr = setup();
        wr.remove_request(&hash(99));
        // No panic; nothing to assert beyond that.
    }

    // -----------------------------------------------------------------
    // release_lock
    // -----------------------------------------------------------------

    #[test]
    fn release_lock_unlocks_the_inflight_tranche() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, h, near(100));
        let _ = wr.amount_of_matured_tranches(10, h);

        wr.release_lock(&h);

        let v = vec_at(&wr, &h).unwrap();
        assert_eq!(v.iter().filter(|l| l.is_locked()).count(), 0);
        assert_eq!(v[0].as_inner_unchecked().withdrawal_amount, near(100));
    }

    #[test]
    fn release_lock_lets_a_subsequent_collapse_re_lock() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, h, near(100));
        let _ = wr.amount_of_matured_tranches(10, h);
        wr.release_lock(&h);

        // After unlocking, a fresh withdraw can collapse again. The tranche
        // is still matured (epoch=5, current=10), so amount is 100.
        let amount = wr.amount_of_matured_tranches(10, h);
        assert_eq!(amount, near(100));
    }

    #[test]
    fn release_lock_no_op_when_nothing_locked() {
        let mut wr = setup();
        let h = hash(1);
        wr.append_request(10, h, near(100));
        wr.release_lock(&h); // should not panic
        let v = vec_at(&wr, &h).unwrap();
        assert!(!v[0].is_locked());
    }

    #[test]
    fn release_lock_no_op_for_missing_hash() {
        let mut wr = setup();
        wr.release_lock(&hash(99)); // should not panic
    }

    // -----------------------------------------------------------------
    // End-to-end: full lifecycle of a partial-refund retry
    // -----------------------------------------------------------------

    #[test]
    fn end_to_end_partial_refund_then_full_drain() {
        let mut wr = setup();
        let h = hash(1);

        // Unstake at epoch 5, mature at 9.
        wr.append_request(5, h, near(100));
        assert_eq!(wr.amount_of_matured_tranches(10, h), near(100));

        // Simulate a partial refund: in-flight tranche shrinks to 30 with
        // residual=70 already-held-as-wNEAR; lock released so the user can
        // retry.
        {
            let inflight = wr.locked_tranche_mut(&h);
            inflight.withdrawal_amount = near(30);
            inflight.wnear_residual = near(70);
        }
        wr.release_lock(&h);

        // User retries withdraw — residual stays matured (epoch unchanged),
        // so it gets re-collapsed and re-locked at the same amount.
        assert_eq!(wr.amount_of_matured_tranches(10, h), near(30));
        let inflight = wr.locked_tranche(&h);
        assert_eq!(inflight.wnear_residual, near(70));

        // Full success this time.
        wr.remove_request(&h);
        assert!(vec_at(&wr, &h).is_none());
    }

    // -----------------------------------------------------------------
    // Independent tranches survive an in-flight withdrawal
    // -----------------------------------------------------------------

    #[test]
    fn unstake_during_withdrawal_does_not_disturb_inflight_tranche() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, h, near(100));
        let inflight_amount = wr.amount_of_matured_tranches(10, h);
        assert_eq!(inflight_amount, near(100));

        // Concurrent unstake at epoch 10 — same epoch as the in-flight
        // tranche's max_epoch (5), but this is a fresh unlocked tranche.
        wr.append_request(10, h, near(50));

        let inflight = wr.locked_tranche(&h);
        assert_eq!(
            inflight.withdrawal_amount,
            near(100),
            "in-flight tranche must not absorb the new unstake"
        );

        // The new tranche lives separately, unlocked.
        let v = vec_at(&wr, &h).unwrap();
        let unlocked: Vec<&Tranche> = v
            .iter()
            .filter(|l| !l.is_locked())
            .map(Lock::as_inner_unchecked)
            .collect();
        assert_eq!(unlocked.len(), 1);
        assert_eq!(unlocked[0].withdrawal_amount, near(50));
        assert_eq!(unlocked[0].unstake_epoch, 10);

        // Settle the in-flight withdrawal: tranche removed, the new one
        // remains queued.
        wr.remove_request(&h);
        let v = vec_at(&wr, &h).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].as_inner_unchecked().withdrawal_amount, near(50));
    }

    // -----------------------------------------------------------------
    // Different message hashes are isolated
    // -----------------------------------------------------------------

    #[test]
    fn distinct_hashes_are_isolated() {
        let mut wr = setup();
        let h1 = hash(1);
        let h2 = hash(2);

        wr.append_request(5, h1, near(100));
        wr.append_request(5, h2, near(200));

        assert_eq!(wr.amount_of_matured_tranches(10, h1), near(100));

        // h2 is unaffected by h1's locked tranche.
        assert_eq!(wr.amount_of_matured_tranches(10, h2), near(200));

        // h1's lock check is local to h1.
        wr.remove_request(&h1);
        assert!(vec_at(&wr, &h1).is_none());
        assert!(vec_at(&wr, &h2).is_some());
    }
}
