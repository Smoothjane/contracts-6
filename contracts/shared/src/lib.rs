//! Helpers shared by every StellarPOS contract.
//!
//! This crate is linked into each contract as a plain `rlib`; it does not
//! deploy on its own. It holds the pieces that must behave identically across
//! contracts: basis-point fee math and storage TTL management.

#![no_std]

use soroban_sdk::{Env, IntoVal, Val};

/// 100% expressed in basis points.
pub const BPS_DENOMINATOR: u32 = 10_000;

/// Hard ceiling on any protocol fee: 10%.
pub const MAX_FEE_BPS: u32 = 1_000;

/// Ledgers close roughly every 5 seconds, so one day is ~17,280 ledgers.
pub const DAY_IN_LEDGERS: u32 = 17_280;

/// Instance storage (config, counters) is bumped to ~30 days whenever it
/// drops below ~29 days.
pub const INSTANCE_BUMP: u32 = 30 * DAY_IN_LEDGERS;
pub const INSTANCE_THRESHOLD: u32 = INSTANCE_BUMP - DAY_IN_LEDGERS;

/// Persistent records (payments, escrows, balances) live ~90 days between
/// touches and are bumped back up whenever they are read or written.
pub const PERSISTENT_BUMP: u32 = 90 * DAY_IN_LEDGERS;
pub const PERSISTENT_THRESHOLD: u32 = PERSISTENT_BUMP - DAY_IN_LEDGERS;

/// Returns `amount * bps / 10_000`, rounded toward zero.
///
/// Returns `None` on overflow or when `bps` exceeds 100%, so callers can map
/// it to their own contract error instead of panicking.
pub fn bps_of(amount: i128, bps: u32) -> Option<i128> {
    if bps > BPS_DENOMINATOR {
        return None;
    }
    amount
        .checked_mul(bps as i128)?
        .checked_div(BPS_DENOMINATOR as i128)
}

/// Splits `amount` into `(fee, net)` where `fee = bps_of(amount, bps)`.
pub fn split_fee(amount: i128, bps: u32) -> Option<(i128, i128)> {
    let fee = bps_of(amount, bps)?;
    Some((fee, amount.checked_sub(fee)?))
}

/// Keeps the calling contract's instance storage alive.
pub fn extend_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_THRESHOLD, INSTANCE_BUMP);
}

/// Keeps one persistent entry alive. Call after every read or write of a
/// long-lived record so active data never gets archived.
pub fn extend_persistent<K>(env: &Env, key: &K)
where
    K: IntoVal<Env, Val>,
{
    env.storage()
        .persistent()
        .extend_ttl(key, PERSISTENT_THRESHOLD, PERSISTENT_BUMP);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bps_rounds_toward_zero() {
        assert_eq!(bps_of(1_000, 250), Some(25));
        assert_eq!(bps_of(999, 1), Some(0));
        assert_eq!(bps_of(0, 500), Some(0));
    }

    #[test]
    fn bps_rejects_more_than_one_hundred_percent() {
        assert_eq!(bps_of(100, 10_001), None);
        assert_eq!(bps_of(100, 10_000), Some(100));
    }

    #[test]
    fn bps_reports_overflow_instead_of_panicking() {
        assert_eq!(bps_of(i128::MAX, 2), None);
    }

    #[test]
    fn split_fee_conserves_the_total() {
        let (fee, net) = split_fee(12_345_678, 275).unwrap();
        assert_eq!(fee + net, 12_345_678);
        assert_eq!(fee, 339_506);
    }
}
