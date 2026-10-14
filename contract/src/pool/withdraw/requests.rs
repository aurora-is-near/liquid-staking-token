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
    pub(crate) requests: IterableMap<CryptoHash, Vec<Lock<Tranche>>>,
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
    pub(crate) fn locked_tranche(&self, hash: &CryptoHash) -> &Tranche {
        self.requests
            .get(hash)
            .unwrap_or_else(|| env::panic_str("No withdrawal for the given hash"))
            .iter()
            .find_map(Lock::as_locked)
            .unwrap_or_else(|| env::panic_str("The user withdrawal should be locked at this point"))
    }

    pub(crate) fn locked_tranche_mut(&mut self, hash: &CryptoHash) -> &mut Tranche {
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
    /// otherwise. Lets settlement callbacks and the admin escape-hatch
    /// `force_release_lock` distinguish a real recovery from a no-op for
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
    /// the `storage_deposit` step.
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
    pub(crate) const fn is_matured(&self, current_epoch: u64) -> bool {
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
    pub(crate) hash: String,
    pub(crate) tranches: Vec<Tranche>,
}
