use super::*;
use crate::pool::unstake::UnstakeMessage;
use crate::pool::{Tranche, WithdrawalRequests};
use defuse_near_utils::Lock;
use near_sdk::borsh::BorshSerialize;
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::{BorshStorageKey, CryptoHash, testing_env};

const MAX_LIMIT: usize = 100;
const UNSTAKE_COOLDOWN_PERIOD: u64 = 4;

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
// ----------------------pub(crate) pub(crate) -------------------------------------------

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
