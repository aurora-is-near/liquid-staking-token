//! 256-bit intermediate arithmetic for exchange-rate and fee math.

/// Returns `floor((a * b) / c)` using 256-bit intermediate precision.
///
/// Panics if `c == 0` or if the final result does not fit in a `u128`.
pub fn mul_div_floor(a: u128, b: u128, c: u128) -> u128 {
    assert!(c != 0, "Division by zero in mul_div_floor");

    if let Some(product) = a.checked_mul(b) {
        return product / c;
    }

    let (hi, lo) = widening_mul_u128(a, b);
    div_256_by_128_floor(hi, lo, c)
}

/// Computes `a * b` as a 256-bit number split into (high, low) u128 halves.
const fn widening_mul_u128(a: u128, b: u128) -> (u128, u128) {
    const MASK: u128 = u64::MAX as u128;
    let a_lo = a & MASK;
    let a_hi = a >> 64;
    let b_lo = b & MASK;
    let b_hi = b >> 64;

    let ll = a_lo * b_lo;
    let lh = a_lo * b_hi;
    let hl = a_hi * b_lo;
    let hh = a_hi * b_hi;

    // Accumulate the 128-bit middle terms (`lh + hl`). This sum can exceed a
    // `u128`, so we track the carry explicitly.
    let (mid_sum, mid_carry) = lh.overflowing_add(hl);
    // `mid_sum << 64` contributes to the low word; its top 64 bits roll into
    // the high word alongside the 129-bit carry.
    let mid_shifted = mid_sum << 64;
    let (low, low_carry) = ll.overflowing_add(mid_shifted);
    let high = hh
        .wrapping_add(mid_sum >> 64)
        .wrapping_add((mid_carry as u128) << 64)
        .wrapping_add(low_carry as u128);
    (high, low)
}

/// Long-division of the 256-bit dividend `(hi, lo)` by `divisor`, returning
/// the floored quotient. Panics if the quotient does not fit in a `u128`.
fn div_256_by_128_floor(hi: u128, lo: u128, divisor: u128) -> u128 {
    assert!(
        hi < divisor,
        "Quotient overflow in mul_div_floor: result does not fit in u128"
    );
    // `rem` is the working remainder (< divisor at the top of each iteration).
    // When a left-shift moves its MSB out of the `u128`, we track the lost bit
    // via `carry` — logically, the effective remainder is `(carry << 128) | rem`.
    let mut rem = hi;
    let mut quot: u128 = 0;
    for i in (0..128).rev() {
        let carry = rem >> 127;
        rem = (rem << 1) | ((lo >> i) & 1);
        quot <<= 1;
        // With `rem_prev < divisor < 2^128`, the effective remainder after
        // the shift is `< 2 * divisor`, so at most one subtraction brings us
        // back under `divisor`.
        if carry == 1 || rem >= divisor {
            rem = rem.wrapping_sub(divisor);
            quot |= 1;
        }
    }
    quot
}

#[cfg(test)]
mod tests {
    use super::mul_div_floor;

    #[test]
    fn basic() {
        assert_eq!(mul_div_floor(10, 10, 5), 20);
        assert_eq!(mul_div_floor(0, 123, 7), 0);
        assert_eq!(mul_div_floor(7, 0, 3), 0);
    }

    #[test]
    fn no_overflow_path() {
        let a = 10u128.pow(24);
        let b = 5u128;
        let c = 2u128;
        assert_eq!(mul_div_floor(a, b, c), a * 5 / 2);
    }

    #[test]
    fn overflow_path_exchange_rate() {
        let near_in: u128 = 10u128.pow(33);
        let total_supply: u128 = 10u128.pow(33);
        let total_staked: u128 = 10u128.pow(33);
        // 1e33 * 1e33 overflows u128 but the result equals 1e33.
        assert_eq!(mul_div_floor(near_in, total_supply, total_staked), near_in);
    }

    #[test]
    fn overflow_path_rounds_down() {
        // floor((a*b)/c): a=u128::MAX, b=u128::MAX, c=u128::MAX gives u128::MAX.
        assert_eq!(
            mul_div_floor(u128::MAX, u128::MAX, u128::MAX),
            u128::MAX
        );
    }

    #[test]
    fn reward_bearing_growth() {
        // Supply 100, backing 110 (10% rewards). Staking 10 NEAR should mint ~9.09 LST.
        let mint = mul_div_floor(10, 100, 110);
        assert_eq!(mint, 9);
    }

    #[test]
    #[should_panic(expected = "Division by zero")]
    fn div_by_zero() {
        mul_div_floor(1, 1, 0);
    }

    #[test]
    #[should_panic(expected = "Quotient overflow")]
    fn quotient_overflow() {
        mul_div_floor(u128::MAX, u128::MAX, 1);
    }
}
