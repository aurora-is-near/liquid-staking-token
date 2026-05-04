use defuse_near_utils::Lock;
use near_sdk::store::IterableMap;
use near_sdk::{CryptoHash, IntoStorageKey, NearToken, env, near, require};

/// Upper bound on the number of entries returned by a single
/// [`WithdrawalRequests::get_withdrawal_requests`] call. Caps the cost (gas +
/// borsh-decode + json-encode) of any one view call regardless of the
/// caller-supplied `limit`.
const MAX_LIMIT: usize = 100;
const UNSTAKE_COOLDOWN_PERIOD: u64 = 4;

/// Pending withdrawal queue, keyed by the hash of the originating
/// [`UnstakeMessage`](crate::pool::UnstakeMessage).
///
/// Each `hash` maps to a list of independent [`Tranche`]s. Every successful
/// `on_unstake` appends one — folding into a same-epoch sibling and collapsing
/// already-matured tranches to keep the vec small — and `withdraw` collapses
/// every currently-matured tranche under the hash into a single in-flight
/// delivery.
///
/// # Invariants
///
/// * **At most one locked tranche per `hash`.** It represents the
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

    /// Returns the number of distinct `hash` entries currently in the
    /// queue.
    ///
    /// One "entry" corresponds to one queue slot in the underlying
    /// [`IterableMap`] — i.e. one `(hash, Vec<Lock<Tranche>>)`
    /// pair — regardless of how many tranches sit under that hash. Useful as
    /// a fuel gauge alongside any storage-bloat cap and as a stable total
    /// for indexers paginating through [`Self::get_withdrawal_requests`].
    pub(crate) fn len(&self) -> u32 {
        self.requests.len()
    }

    /// Records a successful unstake under `hash`. Bounds the queue's vec
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
        hash: CryptoHash,
        amount: NearToken,
    ) {
        let tranches = self.requests.entry(hash).or_default();

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

    /// Sweeps all matured, unlocked tranches under `hash` and merges them
    /// into a single in-flight (locked) tranche, leaving any immature tranches
    /// untouched. Returns the total claimable amount carried by the in-flight
    /// tranche. Panics if a withdrawal is already in flight (any tranche already locked).
    pub(crate) fn amount_of_matured_tranches(
        &mut self,
        current_epoch: u64,
        hash: CryptoHash,
    ) -> NearToken {
        let Some(tranches) = self.requests.get_mut(&hash) else {
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

    /// Returns the in-flight (locked) tranche under `hash`. There is at
    /// most one per `hash` because `collapse_matured_tranches` rejects
    /// new sweeps while a prior tranche is still locked. Panics if missing.
    pub(super) fn locked_tranche(&self, hash: &CryptoHash) -> &Tranche {
        self.requests
            .get(hash)
            .unwrap_or_else(|| env::panic_str("No withdrawal for the given hash"))
            .iter()
            .find_map(Lock::as_locked)
            .unwrap_or_else(|| env::panic_str("The user withdrawal should be locked at this point"))
    }

    pub(super) fn locked_tranche_mut(&mut self, hash: &CryptoHash) -> &mut Tranche {
        self.requests
            .get_mut(hash)
            .unwrap_or_else(|| env::panic_str("No withdrawal for the given hash"))
            .iter_mut()
            .find_map(Lock::as_locked_mut)
            .unwrap_or_else(|| env::panic_str("The user withdrawal should be locked at this point"))
    }

    /// Drops the in-flight (locked) tranche from `hash`. Any unlocked
    /// (queued, possibly non-matured) tranches under the same hash are
    /// preserved. If no tranches remain, the queue entry is removed entirely.
    /// No-op if no tranche is locked.
    pub(crate) fn remove_request(&mut self, hash: &CryptoHash) {
        let Some(tranches) = self.requests.get_mut(hash) else {
            return;
        };

        tranches.retain(|tranche| !tranche.is_locked());

        if tranches.is_empty() {
            self.requests.remove(hash);
        }
    }

    /// Unlocks the in-flight tranche under `hash` so a future `withdraw`
    /// can retry. No-op if no tranche is locked (e.g. the in-flight tranche
    /// was already removed by a successful full withdrawal, or no entry
    /// exists for `hash` at all).
    ///
    /// Returns `true` if a locked tranche was found and unlocked; `false`
    /// otherwise. Lets callers — both the standard tail-of-chain
    /// `remove_lock` callback and the admin escape-hatch
    /// `force_release_lock` — distinguish a real recovery from a no-op for
    /// logging/telemetry without leaking the internal vec layout.
    pub(crate) fn release_lock(&mut self, hash: &CryptoHash) -> bool {
        let Some(tranches) = self.requests.get_mut(hash) else {
            return false;
        };

        tranches
            .iter_mut()
            .find(|t| t.is_locked())
            .is_some_and(|tranche| {
                tranche.force_unlock();
                true
            })
    }

    /// Returns a snapshot of every [`Tranche`] currently queued under
    /// `hash`, or `None` if the hash has no entry in the queue.
    ///
    /// The snapshot is a flat list, in the order tranches are stored inside
    /// the underlying vec (i.e. unspecified — `swap_remove` during
    /// matured-collapse can rearrange). Both unlocked and the (at most one)
    /// locked tranche are returned; callers cannot distinguish the in-flight
    /// tranche through this view since the [`Lock`] wrapper is intentionally
    /// stripped before serialization. Use this for observability /
    /// frontend-facing displays of a single user's pending withdrawals.
    pub(crate) fn get_withdrawal_request_tranches(
        &self,
        hash: &CryptoHash,
    ) -> Option<Vec<Tranche>> {
        self.requests.get(hash).map(|tranches| {
            tranches
                .iter()
                .map(Lock::as_inner_unchecked)
                .copied()
                .collect()
        })
    }

    /// Paginated view of every queue entry, suitable for indexers / admin
    /// dashboards.
    ///
    /// `skip` and `limit` follow the standard offset/limit pagination
    /// pattern; the returned slice is `[skip, skip + min(limit, MAX_LIMIT))`
    /// of the iteration order yielded by the underlying [`IterableMap`].
    /// Each entry packages the `hash` (base58-encoded for human/JSON
    /// readability) and the full tranche list under that hash.
    ///
    /// # Caveats
    ///
    /// * **`MAX_LIMIT` clamps `limit` silently.** A caller asking for `limit
    ///   = 1000` gets at most `MAX_LIMIT` rows back, with no signal that more
    ///   exist. Pair with [`Self::len`] when stable totals are needed.
    /// * **Iteration order is not stable across removals.** `IterableMap`
    ///   uses swap-remove on its key vector, so deleting an entry can shift
    ///   later entries' positions. Callers that paginate across multiple
    ///   calls should be prepared for entries to skip or repeat if the queue
    ///   is mutated mid-traversal.
    pub(crate) fn get_withdrawal_requests(
        &self,
        skip: usize,
        limit: usize,
    ) -> Vec<WithdrawalRequest> {
        self.requests
            .iter()
            .skip(skip)
            .take(limit.min(MAX_LIMIT))
            .map(|(hash, locks)| {
                let hash = near_sdk::bs58::encode(hash).into_string();
                let tranches = locks
                    .iter()
                    .map(Lock::as_inner_unchecked)
                    .copied()
                    .collect();

                WithdrawalRequest { hash, tranches }
            })
            .collect()
    }

    pub(crate) fn get_hashes_available_for_withdrawal(
        &self,
        skip: usize,
        limit: usize,
    ) -> Vec<CryptoHash> {
        let current_epoch = env::epoch_height();

        self.requests
            .iter()
            .filter_map(|(hash, tranches)| {
                let is_any_tranche_matured = tranches
                    .iter()
                    .any(|t| t.get().is_some_and(|t| t.is_matured(current_epoch)));

                if is_any_tranche_matured {
                    Some(*hash)
                } else {
                    None
                }
            })
            .skip(skip)
            .take(limit.min(MAX_LIMIT))
            .collect()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[near(serializers = [borsh, json])]
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

impl Tranche {
    pub(crate) fn new(withdrawal_amount: NearToken, unstake_epoch: u64) -> Self {
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

/// JSON-serialized view of a single queue entry, returned by
/// [`WithdrawalRequests::get_withdrawal_requests`].
///
/// * `hash` — base58 encoding of the on-chain [`CryptoHash`] key. Stored
///   as a string because raw 32-byte keys do not survive JSON round-trips.
/// * `tranches` — every tranche queued under that hash, in storage order.
///   Locked / in-flight tranches are included alongside queued ones; the
///   distinction is intentionally hidden so this struct is a pure
///   observability surface.
#[near(serializers = [json])]
pub struct WithdrawalRequest {
    hash: String,
    tranches: Vec<Tranche>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::unstake::UnstakeMessage;
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

    /// Same as [`setup`], but pins `env::epoch_height()` to `epoch`. Use this
    /// for tests that hit `get_hashes_available_for_withdrawal`, which reads
    /// the current epoch from the VM context (other methods take the epoch
    /// as a parameter and don't need this).
    fn setup_at(epoch: u64) -> WithdrawalRequests {
        testing_env!(VMContextBuilder::new().epoch_height(epoch).build());
        WithdrawalRequests::new(StorageKey::Requests)
    }

    /// Builds a deterministic `UnstakeMessage` from `seed`. Each seed yields
    /// a message that hashes to a unique `hash`, so tests can reference
    /// distinct queue entries by varying the seed.
    fn unstake_msg(seed: u8) -> UnstakeMessage {
        use crate::pool::unstake::WithdrawTokens;
        UnstakeMessage {
            receiver_id: format!("user{seed}.near").parse().unwrap(),
            withdraw_tokens: WithdrawTokens::Native,
        }
    }

    /// Hash that `append_request(_, hash(seed), _)` will key the entry
    /// under. Useful for the lookup-side methods (`amount_of_matured_tranches`,
    /// `locked_tranche`, etc.) that still take a `CryptoHash`.
    fn hash(seed: u8) -> CryptoHash {
        unstake_msg(seed)
            .hash()
            .expect("borsh-serialize UnstakeMessage")
    }

    fn near(yocto: u128) -> NearToken {
        NearToken::from_yoctonear(yocto)
    }

    /// Direct tranche-vec accessor for assertions (bypasses the
    /// `locked_tranche*` public surface since several tests need to inspect
    /// non-locked tranches).
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

        wr.append_request(10, hash(1), near(100));

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

        wr.append_request(10, hash(1), near(100));
        wr.append_request(10, hash(1), near(50));

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
        wr.append_request(10, hash(1), near(100));
        wr.append_request(11, hash(1), near(50));

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
        wr.append_request(10, hash(1), near(100));
        wr.append_request(14, hash(1), near(70));

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

        wr.append_request(5, hash(1), near(100));
        wr.append_request(6, hash(1), near(50));
        // both above are matured at epoch 10 (cooldown = 4)
        wr.append_request(10, hash(1), near(30));

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
        wr.append_request(5, hash(1), near(100));
        let _ = wr.amount_of_matured_tranches(10, h); // locks the merged tranche

        // A new unstake while a withdrawal is mid-flight.
        wr.append_request(10, hash(1), near(40));

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

        wr.append_request(10, hash(1), near(100));
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

        wr.append_request(5, hash(1), near(100));
        wr.append_request(6, hash(1), near(50));

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

        wr.append_request(5, hash(1), near(100)); // matured at epoch 9
        wr.append_request(8, hash(1), near(20)); // matured at epoch 12

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

        wr.append_request(5, hash(1), near(100));
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
        wr.append_request(5, hash(1), near(100));
        wr.append_request(6, hash(1), near(50));

        {
            let tranches = wr.requests.get_mut(&h).unwrap();
            for lock in tranches.iter_mut() {
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

        wr.append_request(5, hash(1), near(100));
        let _ = wr.amount_of_matured_tranches(10, h);

        let t = wr.locked_tranche(&h);
        assert_eq!(t.withdrawal_amount, near(100));
    }

    #[test]
    fn locked_tranche_finds_inflight_among_unlocked_tranches() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, hash(1), near(100));
        let _ = wr.amount_of_matured_tranches(10, h);
        // Another unstake lands while withdraw is mid-flight.
        wr.append_request(10, hash(1), near(33));

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
        wr.append_request(10, hash(1), near(100));
        let _ = wr.locked_tranche(&h);
    }

    // -----------------------------------------------------------------
    // remove_request
    // -----------------------------------------------------------------

    #[test]
    fn remove_request_drops_only_locked_tranche() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, hash(1), near(100));
        let _ = wr.amount_of_matured_tranches(10, h); // locks 100
        wr.append_request(10, hash(1), near(40)); // unlocked tranche at epoch 10

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

        wr.append_request(5, hash(1), near(100));
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

        wr.append_request(10, hash(1), near(100));
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

        wr.append_request(5, hash(1), near(100));
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

        wr.append_request(5, hash(1), near(100));
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
        wr.append_request(10, hash(1), near(100));
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
        wr.append_request(5, hash(1), near(100));
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

        wr.append_request(5, hash(1), near(100));
        let inflight_amount = wr.amount_of_matured_tranches(10, h);
        assert_eq!(inflight_amount, near(100));

        // Concurrent unstake at epoch 10 — same epoch as the in-flight
        // tranche's max_epoch (5), but this is a fresh unlocked tranche.
        wr.append_request(10, hash(1), near(50));

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

        wr.append_request(5, hash(1), near(100));
        wr.append_request(5, hash(2), near(200));

        assert_eq!(wr.amount_of_matured_tranches(10, h1), near(100));

        // h2 is unaffected by h1's locked tranche.
        assert_eq!(wr.amount_of_matured_tranches(10, h2), near(200));

        // h1's lock check is local to h1.
        wr.remove_request(&h1);
        assert!(vec_at(&wr, &h1).is_none());
        assert!(vec_at(&wr, &h2).is_some());
    }

    // -----------------------------------------------------------------
    // len
    // -----------------------------------------------------------------

    #[test]
    fn len_is_zero_on_a_fresh_queue() {
        let wr = setup();
        assert_eq!(wr.len(), 0);
    }

    #[test]
    fn len_counts_distinct_hashes_not_tranches() {
        let mut wr = setup();

        // Two appends under msg #1 collapse into a single entry but produce
        // two tranches in the same vec; this still counts as ONE entry.
        wr.append_request(10, hash(1), near(100));
        wr.append_request(11, hash(1), near(50));
        assert_eq!(wr.len(), 1);

        wr.append_request(10, hash(2), near(200));
        assert_eq!(wr.len(), 2);
    }

    #[test]
    fn len_decrements_when_an_entry_is_fully_drained() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, hash(1), near(100));
        let _ = wr.amount_of_matured_tranches(10, h);
        assert_eq!(wr.len(), 1);

        wr.remove_request(&h);
        assert_eq!(wr.len(), 0);
    }

    #[test]
    fn len_unchanged_when_remove_request_only_drops_locked_tranche() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, hash(1), near(100));
        let _ = wr.amount_of_matured_tranches(10, h);
        wr.append_request(10, hash(1), near(40)); // unlocked sibling

        // Removing only the in-flight tranche leaves the entry alive.
        wr.remove_request(&h);
        assert_eq!(wr.len(), 1);
    }

    // -----------------------------------------------------------------
    // release_lock — return value semantics
    // -----------------------------------------------------------------

    #[test]
    fn release_lock_returns_true_when_it_unlocks() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, hash(1), near(100));
        let _ = wr.amount_of_matured_tranches(10, h);

        assert!(wr.release_lock(&h));

        // And the tranche is actually unlocked afterward.
        let v = vec_at(&wr, &h).unwrap();
        assert!(!v.iter().any(Lock::is_locked));
    }

    #[test]
    fn release_lock_returns_false_when_entry_has_no_lock() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(10, hash(1), near(100));
        assert!(!wr.release_lock(&h));
    }

    #[test]
    fn release_lock_returns_false_for_missing_hash() {
        let mut wr = setup();
        assert!(!wr.release_lock(&hash(99)));
    }

    #[test]
    fn release_lock_is_idempotent() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, hash(1), near(100));
        let _ = wr.amount_of_matured_tranches(10, h);

        assert!(wr.release_lock(&h));
        // Second call finds nothing locked → false, no panic.
        assert!(!wr.release_lock(&h));
    }

    // -----------------------------------------------------------------
    // get_withdrawal_request_tranches
    // -----------------------------------------------------------------

    #[test]
    fn get_withdrawal_request_tranches_returns_none_for_missing_hash() {
        let wr = setup();
        assert!(wr.get_withdrawal_request_tranches(&hash(99)).is_none());
    }

    #[test]
    fn get_withdrawal_request_tranches_returns_all_tranches() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(10, hash(1), near(100));
        wr.append_request(11, hash(1), near(50));

        let tranches = wr.get_withdrawal_request_tranches(&h).unwrap();
        assert_eq!(tranches.len(), 2);

        let mut amounts: Vec<u128> = tranches
            .iter()
            .map(|t| t.withdrawal_amount.as_yoctonear())
            .collect();
        amounts.sort_unstable();
        assert_eq!(amounts, vec![50, 100]);
    }

    #[test]
    fn get_withdrawal_request_tranches_includes_locked_tranche() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, hash(1), near(100));
        let _ = wr.amount_of_matured_tranches(10, h); // locks the merged tranche
        wr.append_request(10, hash(1), near(40)); // unlocked sibling

        let tranches = wr.get_withdrawal_request_tranches(&h).unwrap();
        // Both the locked in-flight tranche and the unlocked sibling are
        // returned — the lock state isn't observable through this view.
        assert_eq!(tranches.len(), 2);

        let mut amounts: Vec<u128> = tranches
            .iter()
            .map(|t| t.withdrawal_amount.as_yoctonear())
            .collect();
        amounts.sort_unstable();
        assert_eq!(amounts, vec![40, 100]);
    }

    #[test]
    fn get_withdrawal_request_tranches_returns_none_after_full_drain() {
        let mut wr = setup();
        let h = hash(1);

        wr.append_request(5, hash(1), near(100));
        let _ = wr.amount_of_matured_tranches(10, h);
        wr.remove_request(&h);

        assert!(wr.get_withdrawal_request_tranches(&h).is_none());
    }

    // -----------------------------------------------------------------
    // get_withdrawal_requests — pagination
    // -----------------------------------------------------------------

    #[test]
    fn get_withdrawal_requests_empty_queue_returns_empty_vec() {
        let wr = setup();
        let page = wr.get_withdrawal_requests(0, 10);
        assert!(page.is_empty());
    }

    #[test]
    fn get_withdrawal_requests_returns_every_entry() {
        let mut wr = setup();
        wr.append_request(10, hash(1), near(100));
        wr.append_request(10, hash(2), near(200));
        wr.append_request(10, hash(3), near(300));

        let page = wr.get_withdrawal_requests(0, 100);
        assert_eq!(page.len(), 3);

        let mut totals: Vec<u128> = page
            .iter()
            .map(|r| {
                r.tranches
                    .iter()
                    .map(|t| t.withdrawal_amount.as_yoctonear())
                    .sum()
            })
            .collect();
        totals.sort_unstable();
        assert_eq!(totals, vec![100, 200, 300]);
    }

    #[test]
    fn get_withdrawal_requests_skip_and_limit_apply() {
        let mut wr = setup();
        for i in 1..=5u8 {
            wr.append_request(10, hash(i), near(u128::from(i) * 10));
        }

        let head = wr.get_withdrawal_requests(0, 2);
        assert_eq!(head.len(), 2);

        let tail = wr.get_withdrawal_requests(3, 100);
        assert_eq!(tail.len(), 2, "skipping 3 of 5 leaves 2");

        let middle = wr.get_withdrawal_requests(1, 2);
        assert_eq!(middle.len(), 2);

        // Skip past the end yields empty.
        let past = wr.get_withdrawal_requests(10, 5);
        assert!(past.is_empty());
    }

    #[test]
    #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
    fn get_withdrawal_requests_clamps_limit_to_max_limit() {
        let mut wr = setup();
        // Fill MAX_LIMIT + 5 entries.
        for i in 0..(MAX_LIMIT + 5) {
            wr.append_request(10, hash(i as u8), near(1));
        }
        assert_eq!(wr.len() as usize, MAX_LIMIT + 5);

        // Asking for everything yields at most MAX_LIMIT rows.
        let page = wr.get_withdrawal_requests(0, MAX_LIMIT + 100);
        assert_eq!(page.len(), MAX_LIMIT);

        // The remaining rows are reachable via skip.
        let rest = wr.get_withdrawal_requests(MAX_LIMIT, MAX_LIMIT);
        assert_eq!(rest.len(), 5);
    }

    #[test]
    fn get_withdrawal_requests_encodes_hash_as_base58() {
        let mut wr = setup();
        let h = hash(1);
        wr.append_request(10, hash(1), near(100));

        let page = wr.get_withdrawal_requests(0, 1);
        assert_eq!(page.len(), 1);

        let expected = near_sdk::bs58::encode(h).into_string();
        assert_eq!(page[0].hash, expected);
    }

    #[test]
    fn get_withdrawal_requests_includes_all_tranches_per_entry() {
        let mut wr = setup();

        wr.append_request(10, hash(1), near(100));
        wr.append_request(11, hash(1), near(50)); // separate tranche, not folded

        let page = wr.get_withdrawal_requests(0, 1);
        assert_eq!(page[0].tranches.len(), 2);

        let mut amounts: Vec<u128> = page[0]
            .tranches
            .iter()
            .map(|t| t.withdrawal_amount.as_yoctonear())
            .collect();
        amounts.sort_unstable();
        assert_eq!(amounts, vec![50, 100]);
    }

    // -----------------------------------------------------------------
    // get_hashes_available_for_withdrawal
    // -----------------------------------------------------------------

    #[test]
    fn get_hashes_available_for_withdrawal_returns_empty_when_queue_empty() {
        let wr = setup_at(10);
        assert!(wr.get_hashes_available_for_withdrawal(0, 100).is_empty());
    }

    #[test]
    fn get_hashes_available_for_withdrawal_returns_hashes_with_matured_tranches() {
        let mut wr = setup_at(10);

        // Both unstakes' tranches mature at epoch 9 (5 + 4); env epoch is 10,
        // so both hashes are claimable now.
        wr.append_request(5, hash(1), near(100));
        wr.append_request(5, hash(2), near(200));

        let mut got = wr.get_hashes_available_for_withdrawal(0, 100);
        got.sort_unstable();
        let mut want = vec![hash(1), hash(2)];
        want.sort_unstable();
        assert_eq!(got, want);
    }

    #[test]
    fn get_hashes_available_for_withdrawal_excludes_hashes_with_only_non_matured_tranches() {
        let mut wr = setup_at(10);

        // unstake_epoch = 8 → matured at 12; env epoch is 10 → not yet ready.
        wr.append_request(8, hash(1), near(100));

        assert!(wr.get_hashes_available_for_withdrawal(0, 100).is_empty());
    }

    #[test]
    fn get_hashes_available_for_withdrawal_excludes_hash_with_only_locked_tranche() {
        let mut wr = setup_at(10);
        let h = hash(1);

        wr.append_request(5, hash(1), near(100));
        // Lock the merged in-flight tranche. The hash now has only one
        // locked tranche; `get_hashes_available_for_withdrawal` should
        // exclude it because `withdraw_by_hash` would panic with "already
        // in progress".
        let _ = wr.amount_of_matured_tranches(10, h);

        assert!(wr.get_hashes_available_for_withdrawal(0, 100).is_empty());
    }

    #[test]
    fn get_hashes_available_for_withdrawal_includes_hash_with_locked_plus_unlocked_matured() {
        let mut wr = setup_at(10);
        let h = hash(1);

        // Lock the first unstake's merged tranche.
        wr.append_request(5, hash(1), near(100));
        let _ = wr.amount_of_matured_tranches(10, h);

        // A second unstake under the same hash lands as an *unlocked*
        // tranche at epoch 6, which is matured at env epoch 10.
        wr.append_request(6, hash(1), near(40));

        // The unlocked-matured sibling qualifies, even though the locked
        // in-flight tranche under the same hash does not.
        assert_eq!(wr.get_hashes_available_for_withdrawal(0, 100), vec![h]);
    }

    #[test]
    fn get_hashes_available_for_withdrawal_skip_and_limit_apply_to_filtered_stream() {
        let mut wr = setup_at(10);

        // Three matured + one not-matured. Filtered stream has 3 entries.
        wr.append_request(5, hash(1), near(10));
        wr.append_request(8, hash(2), near(20)); // not matured at epoch 10
        wr.append_request(5, hash(3), near(30));
        wr.append_request(5, hash(4), near(40));

        // skip=1, limit=10 → returns 2 of the 3 matured hashes.
        let got = wr.get_hashes_available_for_withdrawal(1, 10);
        assert_eq!(got.len(), 2, "skip applies to the post-filter stream");
        // Whatever 2 we got, neither should be hash(2) (the non-matured one).
        assert!(!got.contains(&hash(2)));

        // limit clamps the count: with skip=0, limit=2 we get 2 of 3.
        let got = wr.get_hashes_available_for_withdrawal(0, 2);
        assert_eq!(got.len(), 2);

        // skip past the filtered end → empty.
        let got = wr.get_hashes_available_for_withdrawal(10, 100);
        assert!(got.is_empty());
    }

    #[test]
    #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
    fn get_hashes_available_for_withdrawal_clamps_limit_to_max_limit() {
        let mut wr = setup_at(10);

        for i in 0..(MAX_LIMIT + 5) {
            let seed = i as u8;
            wr.append_request(5, hash(seed), near(1));
        }
        assert_eq!(wr.len() as usize, MAX_LIMIT + 5);

        // Asking for everything yields at most `MAX_LIMIT` rows.
        let page = wr.get_hashes_available_for_withdrawal(0, MAX_LIMIT + 100);
        assert_eq!(page.len(), MAX_LIMIT);

        // The remainder is reachable via skip.
        let rest = wr.get_hashes_available_for_withdrawal(MAX_LIMIT, MAX_LIMIT);
        assert_eq!(rest.len(), 5);
    }

    #[test]
    fn get_hashes_available_for_withdrawal_observes_current_epoch() {
        // Same data; only the env epoch differs between the two `wr`s.
        let mut early = setup_at(7);
        early.append_request(5, hash(1), near(100));
        // 5 + 4 = 9; env epoch 7 → not yet matured.
        assert!(early.get_hashes_available_for_withdrawal(0, 100).is_empty());

        let mut late = setup_at(15);
        late.append_request(5, hash(1), near(100));
        assert_eq!(
            late.get_hashes_available_for_withdrawal(0, 100),
            vec![hash(1)]
        );
    }

    #[test]
    fn get_hashes_available_for_withdrawal_drops_hash_after_full_drain() {
        let mut wr = setup_at(10);
        let h = hash(1);

        wr.append_request(5, hash(1), near(100));
        let _ = wr.amount_of_matured_tranches(10, h);
        wr.remove_request(&h); // simulates a successful full withdraw

        assert!(wr.get_hashes_available_for_withdrawal(0, 100).is_empty());
    }
}
