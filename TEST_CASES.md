# E2E Test Cases

Complete list of all possible end-to-end test scenarios for the Liquid Staking Token contract.
Existing tests are marked **[covered]**; gaps are marked **[missing]**.

---

## Staking — Native NEAR

| # | Scenario | Status |
|---|----------|--------|
| 1 | Stake → plain `ft_transfer` to self | **[covered]** |
| 2 | Stake → plain `ft_transfer` to another account (bob) | **[covered]** |
| 3 | Stake → plain `ft_transfer` to unregistered account → fails | **[covered]** |
| 4 | Stake → plain `ft_transfer` with `storage_deposit` → registers then transfers | **[covered]** |
| 5 | Stake → `ft_on_transfer` to intents, success | **[covered]** |
| 6 | Stake → `ft_on_transfer` to intents for bob | **[covered]** |
| 7 | Stake → `ft_on_transfer` panics, no `refund_message` → tokens stuck on receiver | **[covered]** |
| 8 | Stake → `ft_on_transfer` panics, `refund_message` → native NEAR refund after cooldown | **[covered]** |
| 9 | Stake → `ft_on_transfer` panics, `refund_message` → wNEAR refund after cooldown | **[covered]** |
| 10 | Stake → `ft_on_transfer` panics, `refund_message` routes through intents to bob | **[covered]** |
| 11 | Stake → `ft_on_transfer` returns partial refund (not panic), `refund_message` present → partial unstake queued | **[covered]** |
| 12 | Stake → `ft_on_transfer` returns partial refund, no `refund_message` → remaining tokens stuck | **[covered]** |
| 13 | Stake with `storage_deposit` > stake amount → fails | **[covered]** |
| 14 | Stake to `receiver_id = current_account_id` (self-stake) | **[covered]** |
| 15 | Stake with 0 attached NEAR → fails | **[covered]** |
| 16 | Stake with only enough for `storage_deposit` (stake_amount = 0) → fails | **[covered]** |

---

## Staking — wNEAR

| # | Scenario | Status |
|---|----------|--------|
| 17 | Stake wNEAR → plain `ft_transfer` to self | **[covered]** |
| 18 | Stake wNEAR → plain `ft_transfer` to bob | **[covered]** |
| 19 | Stake wNEAR → plain `ft_transfer` to unregistered account | **[covered]** |
| 20 | Stake wNEAR → `ft_on_transfer` to intents, success | **[covered]** |
| 21 | Stake wNEAR → `ft_on_transfer` to intents for bob | **[covered]** |
| 22 | Stake wNEAR → `ft_on_transfer` to unregistered intents receiver | **[covered]** |
| 23 | Stake wNEAR → wrong/invalid `StakeMessage` format → wNEAR refunded | **[covered]** |
| 24 | Stake wNEAR → `ft_on_transfer` panics, `refund_message` → wNEAR refund | **[covered]** |
| 25 | Stake wNEAR → `ft_on_transfer` panics, `refund_message` → native refund | **[covered]** |
| 26 | Stake wNEAR → `ft_on_transfer` partial refund + `refund_message` | **[covered]** |
| 27 | Non-wNEAR, non-LST token sent via `ft_on_transfer` → panics | **[covered]** |

---

## Unstaking

| # | Scenario | Status |
|---|----------|--------|
| 28 | Unstake via `ft_transfer_call` on LST → native output to self | **[covered]** |
| 29 | Unstake via `ft_transfer_call` on LST → native output to another account | **[covered]** |
| 30 | Unstake via `ft_transfer_call` on LST → wNEAR output to self | **[covered]** |
| 31 | Unstake via `ft_transfer_call` on LST → wNEAR output to another account | **[covered]** |
| 32 | Unstake via `ft_transfer_call` on LST → wNEAR with `ft_transfer_call` to intents (with `msg`) | **[covered]** |
| 33 | Unstake via `ft_transfer_call` on LST → wNEAR `ft_transfer_call` to bad receiver → partial refund stays in queue | **[covered]** |
| 34 | Unstake via `ft_transfer_call` on LST → wNEAR with `storage_deposit` | **[missing]** |
| 35 | Unstake via intents execute → native output | **[covered]** |
| 36 | Unstake via intents execute → wNEAR with `storage_deposit` | **[covered]** |
| 37 | Unstake via intents execute → wNEAR without `storage_deposit` (not registered) | **[covered]** |
| 38 | Partial unstake (send fewer tokens than owned) → remaining LST preserved | **[covered]** |
| 39 | Two unstakes accumulated into same queue entry, then withdrawn | **[covered]** |
| 40 | Unstake more than staked amount → fails | **[missing]** |
| 41 | Unstake with invalid `UnstakeMessage` format → fails | **[missing]** |
| 42 | Unstake 0 tokens → fails | **[missing]** |

---

## Withdrawal

| # | Scenario | Status |
|---|----------|--------|
| 43 | Withdraw before cooldown passes → fails | **[covered]** |
| 44 | Withdraw for a nonexistent queue entry → fails | **[covered]** |
| 45 | Withdraw with modified `UnstakeMessage` (hash mismatch) → fails | **[missing]** |
| 46 | Withdraw native NEAR after cooldown | **[covered]** |
| 47 | Withdraw wNEAR after cooldown | **[covered]** |
| 48 | Withdraw wNEAR via `ft_transfer_call` (with `msg`) | **[covered]** |
| 49 | Withdraw wNEAR via `ft_transfer_call` with bad `msg` → partial refund → remaining stays in queue | **[covered]** |
| 50 | Withdraw remaining queue entry after partial consume | **[covered]** |
| 51 | Re-unstake with same `UnstakeMessage` after partial wNEAR refund → residual plus new unstake withdraws fully | **[covered]** |
| 52 | Withdraw wNEAR with `storage_deposit` to unregistered account | **[missing]** |
| 53 | Withdraw wNEAR with `storage_deposit` exceeding withdrawal amount → fails | **[missing]** |
| 54 | Withdraw wNEAR to `receiver_id = current_account_id` with `storage_deposit` set → fails | **[missing]** |

---

## Storage Management

| # | Scenario | Status |
|---|----------|--------|
| 55 | `storage_deposit` to register a new account | **[missing]** |
| 56 | `storage_deposit` with `registration_only = true` | **[missing]** |
| 57 | `storage_unregister` with zero balance → succeeds | **[missing]** |
| 58 | `storage_unregister` with non-zero balance, no force → fails | **[missing]** |
| 59 | `storage_unregister` with non-zero balance, `force = true` → burns balance | **[missing]** |
| 60 | `storage_withdraw` → recover excess deposit | **[missing]** |
| 61 | `storage_balance_of` for registered / unregistered accounts | **[missing]** |

---

## NEP-141 Token Operations

| # | Scenario | Status |
|---|----------|--------|
| 62 | `ft_transfer` from alice to bob | **[missing]** |
| 63 | `ft_transfer` to unregistered account → fails | **[missing]** |
| 64 | `ft_transfer_call` from alice to a DeFi contract (not for unstaking) | **[missing]** |
| 65 | `ft_transfer_call` to a contract that returns full refund | **[missing]** |
| 66 | `ft_total_supply` reflects stakes and burns correctly | **[missing]** |
| 67 | `ft_metadata` returns correct fields | **[missing]** |

---

## Multi-user / Concurrent Scenarios

| # | Scenario | Status |
|---|----------|--------|
| 68 | Two users stake and unstake independently (different amounts, different timings) | **[covered]** |
| 69 | Two users stake concurrently → `total_staked_amount` accumulates correctly | **[covered]** |
| 70 | User A unstakes while User B stakes in the same block | **[covered]** |
| 71 | User A fully unstakes while User B still has stake → supply goes to zero mid-life | **[missing]** |

---

## Access Control / Admin

| # | Scenario | Status |
|---|----------|--------|
| 72 | Pause contract → `stake` fails | **[missing]** |
| 73 | Pause contract → `withdraw` fails | **[missing]** |
| 74 | Unpause → operations resume | **[missing]** |
| 75 | Non-owner calls pause → fails | **[missing]** |
| 76 | `get_owner_id` returns correct owner | **[missing]** |
| 77 | `get_staking_key` returns correct validator key | **[missing]** |

---

## Summary

| Area | Covered | Missing | Total |
|------|---------|---------|-------|
| Staking (native) | 16 | 0 | 16 |
| Staking (wNEAR) | 11 | 0 | 11 |
| Unstaking | 11 | 4 | 15 |
| Withdrawal | 8 | 4 | 12 |
| Storage management | 0 | 7 | 7 |
| NEP-141 operations | 0 | 6 | 6 |
| Multi-user | 3 | 1 | 4 |
| Access control | 0 | 6 | 6 |
| **Total** | **49** | **28** | **77** |
