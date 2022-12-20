use crate::fraction;
use crate::to_u128_wrapper;
use crate::transcendental::saturating_powi_high_precision;
use crate::types::{Balance, Fraction};

use num_traits::One;
use primitive_types::{U128, U256, U512};
use sp_arithmetic::Rational128;

pub type Price = Rational128;

/// Calculate the iterated exponential moving average for the given prices.
/// `iterations` is the number of iterations of the EMA to calculate.
/// `prev` is the previous oracle value, `incoming` is the new value to integrate.
/// `smoothing` is the smoothing factor of the EMA.
pub fn iterated_price_ema(iterations: u32, prev: Price, incoming: Price, smoothing: Fraction) -> Price {
    price_weighted_average(prev, incoming, exp_smoothing(smoothing, iterations))
}

/// Calculate the iterated exponential moving average for the given balances.
/// `iterations` is the number of iterations of the EMA to calculate.
/// `prev` is the previous oracle value, `incoming` is the new value to integrate.
/// `smoothing` is the smoothing factor of the EMA.
pub fn iterated_balance_ema(iterations: u32, prev: Balance, incoming: Balance, smoothing: Fraction) -> Balance {
    balance_weighted_average(prev, incoming, exp_smoothing(smoothing, iterations))
}

/// Calculate the iterated exponential moving average for the given volumes.
/// `iterations` is the number of iterations of the EMA to calculate.
/// `prev` is the previous oracle value, `incoming` is the new value to integrate.
/// `smoothing` is the smoothing factor of the EMA.
pub fn iterated_volume_ema(
    iterations: u32,
    prev: (Balance, Balance, Balance, Balance),
    incoming: (Balance, Balance, Balance, Balance),
    smoothing: Fraction,
) -> (Balance, Balance, Balance, Balance) {
    volume_weighted_average(prev, incoming, exp_smoothing(smoothing, iterations))
}

/// Calculate the smoothing factor for a period from a given combination of original smoothing
/// factor and iterations by exponentiating the complement by the iterations.
///
/// Example:
/// `exp_smoothing(0.6, 2) = 1 - (1 - 0.6)^2 = 1 - 0.40^2 = 1 - 0.16 = 0.84`
///
/// ```
/// # use hydra_dx_math::ema::exp_smoothing;
/// # use hydra_dy_math::types::Fraction;
/// assert_eq!(exp_smoothing(Fraction::from_num(0.6), 2), FixedU128::from_num(0.84));
/// ```
pub fn exp_smoothing(smoothing: Fraction, iterations: u32) -> Fraction {
    debug_assert!(smoothing <= Fraction::one());
    let complement = Fraction::one() - smoothing;
    // in order to determine the iterated smoothing factor we exponentiate the complement
    let exp_complement: Fraction = saturating_powi_high_precision(complement, iterations);
    debug_assert!(exp_complement <= Fraction::one());
    Fraction::one() - exp_complement
}

/// Calculates smoothing factor alpha for an exponential moving average based on `period`:
/// `alpha = 2 / (period + 1)`. It leads to the "center of mass" of the EMA corresponding to be the
/// "center of mass" of a `period`-length SMA.
///
/// Possible alternatives for `alpha = 2 / (period + 1)`:
/// + `alpha = 1 - 0.5^(1 / period)` for a half-life of `period` or
/// + `alpha = 1 - 0.5^(2 / period)` to have the same median as a `period`-length SMA.
/// See https://en.wikipedia.org/wiki/Moving_average#Relationship_between_SMA_and_EMA
pub fn smoothing_from_period(period: u64) -> Fraction {
    fraction::frac(2, u128::from(period.max(1)).saturating_add(1))
}

/// Calculate a weighted average for the given prices.
/// `prev` is the previous oracle value, `incoming` is the new value to integrate.
/// `weight` is how much weight to give the new value.
///
/// Note: Rounding is biased towards `prev`.
pub fn price_weighted_average(prev: Price, incoming: Price, weight: Fraction) -> Price {
    debug_assert!(weight <= Fraction::one(), "weight must be <= 1");
    if incoming >= prev {
        rounding_add(prev, multiply(weight, saturating_sub(incoming, prev)), Rounding::Down)
    } else {
        rounding_sub(prev, multiply(weight, saturating_sub(prev, incoming)), Rounding::Up)
    }
}

/// Calculate a weighted average for the given balances.
/// `prev` is the previous oracle value, `incoming` is the new value to integrate.
/// `weight` is how much weight to give the new value.
///
/// Note: Rounding is biased towards `prev`.
pub fn balance_weighted_average(prev: Balance, incoming: Balance, weight: Fraction) -> Balance {
    debug_assert!(weight <= Fraction::one(), "weight must be <= 1");
    if incoming >= prev {
        // Safe to use bare `+` because `weight <= 1` and `a + (b - a) <= b`.
        // Safe to use bare `-` because of the conditional.
        prev + fraction::multiply_by_balance(weight, incoming - prev)
    } else {
        // Safe to use bare `-` because `weight <= 1` and `a - (a - b) >= 0` and the conditional.
        prev - fraction::multiply_by_balance(weight, prev - incoming)
    }
}

/// Calculate a weighted average for the given volumes.
/// `prev` is the previous oracle value, `incoming` is the new value to integrate.
/// `weight` is how much weight to give the new value.
///
/// Note: Just delegates to `balance_weighted_average` under the hood.
/// Note: Rounding is biased towards `prev`.
pub fn volume_weighted_average(
    prev: (Balance, Balance, Balance, Balance),
    incoming: (Balance, Balance, Balance, Balance),
    weight: Fraction,
) -> (Balance, Balance, Balance, Balance) {
    debug_assert!(weight <= Fraction::one(), "weight must be <= 1");
    let (prev_a_in, prev_b_out, prev_a_out, prev_b_in) = prev;
    let (a_in, b_out, a_out, b_in) = incoming;
    (
        balance_weighted_average(prev_a_in, a_in, weight),
        balance_weighted_average(prev_b_out, b_out, weight),
        balance_weighted_average(prev_a_out, a_out, weight),
        balance_weighted_average(prev_b_in, b_in, weight),
    )
}

// Utility functions for working with rational numbers.

/// Subtract `r` from `l` and return a tuple of `U256` for full precision.
/// Saturates if `r >= l`.
pub(super) fn saturating_sub(l: Rational128, r: Rational128) -> (U256, U256) {
    if l.n() == 0 || r.n() == 0 {
        return (l.n().into(), l.d().into());
    }
    let (l_n, l_d, r_n, r_d) = to_u128_wrapper!(l.n(), l.d(), r.n(), r.d());
    // n = l.n * r.d - r.n * l.d
    let n = l_n.full_mul(r_d).saturating_sub(r_n.full_mul(l_d));
    // d = l.d * r.d
    let d = l_d.full_mul(r_d);
    (n, d)
}

/// Multiply a `Fraction` `f` with a rational number of `U256`s, returning a tuple of `U512`s for full
/// precision.
pub(super) fn multiply(f: Fraction, (r_n, r_d): (U256, U256)) -> (U512, U512) {
    debug_assert!(f <= Fraction::ONE);
    if f.is_zero() || r_n.is_zero() {
        return (U512::zero(), U512::one());
    } else if f.is_one() {
        return (r_n.into(), r_d.into());
    }
    // n = l.n * f.to_bits
    let n = r_n.full_mul(U256::from(f.to_bits()));
    // d = l.d * DIV
    let d = r_d.full_mul(U256::from(crate::fraction::DIV));
    (n, d)
}

/// Enum to specify how to round a rational number.
/// `Nearest` rounds both numerator and denominator down.
/// `Down` ensures the output is less than or equal to the input.
/// `Up` ensures the output is greater than or equal to the input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Rounding {
    Nearest,
    Down,
    Up,
}

impl Rounding {
    pub fn to_bias(self, magnitude: u128) -> (u128, u128) {
        match self {
            Rounding::Nearest => (0, 0),
            Rounding::Down => (0, magnitude),
            Rounding::Up => (magnitude, 0),
        }
    }
}

/// Reduce the precision of a 512 bit rational number to 383 bits.
/// The rounding is done by shifting which implicitly rounds down both numerator and denominator.
/// This can effectivly round the complete rational number up or down pseudo-randomly.
/// Specify `rounding` other than `Nearest` to round the whole number up or down.
pub(super) fn round((n, d): (U512, U512), rounding: Rounding) -> (U512, U512) {
    let shift = n.bits().max(d.bits()).saturating_sub(383); // anticipate the saturating_add
    if shift > 0 {
        let min_n = if n.is_zero() { U512::zero() } else { U512::one() };
        let (bias_n, bias_d) = rounding.to_bias(1);
        (
            (n >> shift).saturating_add(bias_n.into()).max(min_n),
            (d >> shift).saturating_add(bias_d.into()).max(U512::one()),
        )
    } else {
        (n, d)
    }
}

/// Round a 512 bit rational number to a 128 bit rational number.
/// The rounding is done by shifting which implicitly rounds down both numerator and denominator.
/// This can effectivly round the complete rational number up or down pseudo-randomly.
/// Specify `rounding` other than `Nearest` to round the whole number up or down.
pub(super) fn round_to_rational((n, d): (U512, U512), rounding: Rounding) -> Rational128 {
    let shift = n.bits().max(d.bits()).saturating_sub(128);
    let (n, d) = if shift > 0 {
        let min_n = if n.is_zero() { 0 } else { 1 };
        let (bias_n, bias_d) = rounding.to_bias(1);
        let shifted_n = (n >> shift).low_u128();
        let shifted_d = (d >> shift).low_u128();
        (
            shifted_n.saturating_add(bias_n).max(min_n),
            shifted_d.saturating_add(bias_d).max(1),
        )
    } else {
        (n.low_u128(), d.low_u128())
    };
    Rational128::from(n, d)
}

/// Add `l` and `r` and round the result to a 128 bit rational number.
/// The precision of `r` is reduced to 383 bits so the multiplications don't saturate.
pub(super) fn rounding_add(l: Rational128, (r_n, r_d): (U512, U512), rounding: Rounding) -> Rational128 {
    if l.n() == 0 {
        return round_to_rational((r_n, r_d), Rounding::Nearest);
    } else if r_n.is_zero() {
        return l;
    }
    let (l_n, l_d) = (U512::from(l.n()), U512::from(l.d()));
    let (r_n, r_d) = round((r_n, r_d), rounding);
    // n = l.n * r.d + r.n * l.d
    let n = l_n.saturating_mul(r_d).saturating_add(r_n.saturating_mul(l_d));
    // d = l.d * r.d
    let d = l_d.saturating_mul(r_d);
    round_to_rational((n, d), rounding)
}

/// Subract `l` and `r` (saturating) and round the result to a 128 bit rational number.
/// The precision of `r` is reduced to 383 bits so the multiplications don't saturate.
pub(super) fn rounding_sub(l: Rational128, (r_n, r_d): (U512, U512), rounding: Rounding) -> Rational128 {
    if l.n() == 0 || r_n.is_zero() {
        return l;
    }
    let (l_n, l_d) = (U512::from(l.n()), U512::from(l.d()));
    let (r_n, r_d) = round((r_n, r_d), rounding);
    // n = l.n * r.d - r.n * l.d
    let n = l_n.saturating_mul(r_d).saturating_sub(r_n.saturating_mul(l_d));
    // d = l.d * r.d
    let d = l_d.saturating_mul(r_d);
    round_to_rational((n, d), rounding)
}
