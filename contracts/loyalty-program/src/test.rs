extern crate std;

use super::*;
use soroban_sdk::{testutils::Address as _, Env};

const UNIT: i128 = 10_000_000;

struct Setup<'a> {
    env: Env,
    client: LoyaltyProgramClient<'a>,
    merchant: Address,
    customer: Address,
}

fn setup() -> Setup<'static> {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(LoyaltyProgram, ());
    let client = LoyaltyProgramClient::new(&env, &id);
    client.initialize(&Address::generate(&env));
    let merchant = Address::generate(&env);
    // 10 points per whole unit spent.
    client.enroll(&merchant, &10, &UNIT);
    Setup {
        customer: Address::generate(&env),
        merchant,
        client,
        env,
    }
}

#[test]
fn merchant_accrues_points_for_in_store_sales() {
    let s = setup();
    let earned = s
        .client
        .accrue(&s.merchant, &s.merchant, &s.customer, &(25 * UNIT));
    assert_eq!(earned, 250);

    let view = s.client.get_account(&s.merchant, &s.customer);
    assert_eq!(view.points, 250);
    assert_eq!(view.lifetime, 250);
    assert_eq!(view.tier, Tier::Bronze);
    assert_eq!(view.to_next_tier, SILVER_THRESHOLD - 250);
}

#[test]
fn tiers_upgrade_automatically_and_boost_earning() {
    let s = setup();
    // 100 units -> 1,000 points -> Silver.
    s.client
        .accrue(&s.merchant, &s.merchant, &s.customer, &(100 * UNIT));
    assert_eq!(
        s.client.get_account(&s.merchant, &s.customer).tier,
        Tier::Silver
    );

    // Silver earns 1.10x: 10 units -> 110 points.
    let earned = s
        .client
        .accrue(&s.merchant, &s.merchant, &s.customer, &(10 * UNIT));
    assert_eq!(earned, 110);
}

#[test]
fn redemption_spends_points_but_never_demotes_tier() {
    let s = setup();
    s.client
        .accrue(&s.merchant, &s.merchant, &s.customer, &(500 * UNIT));
    let view = s.client.get_account(&s.merchant, &s.customer);
    assert_eq!(view.tier, Tier::Gold);

    let remaining = s.client.redeem(&s.merchant, &s.customer, &view.points);
    assert_eq!(remaining, 0);
    let after = s.client.get_account(&s.merchant, &s.customer);
    assert_eq!(after.points, 0);
    assert_eq!(after.tier, Tier::Gold);
}

#[test]
fn over_redemption_fails() {
    let s = setup();
    s.client
        .accrue(&s.merchant, &s.merchant, &s.customer, &UNIT);
    assert_eq!(
        s.client.try_redeem(&s.merchant, &s.customer, &11),
        Err(Ok(LoyaltyError::InsufficientPoints))
    );
}

#[test]
fn only_merchant_or_registered_processor_can_accrue() {
    let s = setup();
    let stranger = Address::generate(&s.env);
    assert_eq!(
        s.client
            .try_accrue(&stranger, &s.merchant, &s.customer, &UNIT),
        Err(Ok(LoyaltyError::Unauthorized))
    );

    let processor = Address::generate(&s.env);
    s.client.set_processor(&processor);
    assert_eq!(
        s.client.accrue(&processor, &s.merchant, &s.customer, &UNIT),
        10
    );
}

#[test]
fn unenrolled_or_inactive_merchant_accrues_nothing() {
    let s = setup();
    let other = Address::generate(&s.env);
    assert_eq!(s.client.accrue(&other, &other, &s.customer, &UNIT), 0);

    s.client.configure(&s.merchant, &10, &UNIT, &false);
    assert_eq!(
        s.client
            .accrue(&s.merchant, &s.merchant, &s.customer, &UNIT),
        0
    );
}

#[test]
fn quote_matches_accrual() {
    let s = setup();
    s.client
        .accrue(&s.merchant, &s.merchant, &s.customer, &(600 * UNIT));
    let quoted = s.client.quote(&s.merchant, &s.customer, &(10 * UNIT));
    let earned = s
        .client
        .accrue(&s.merchant, &s.merchant, &s.customer, &(10 * UNIT));
    assert_eq!(quoted, earned);
    assert_eq!(earned, 125);
}

#[test]
fn rate_validation() {
    let s = setup();
    let m = Address::generate(&s.env);
    assert_eq!(
        s.client.try_enroll(&m, &0, &UNIT),
        Err(Ok(LoyaltyError::InvalidRate))
    );
    assert_eq!(
        s.client.try_enroll(&m, &10, &0),
        Err(Ok(LoyaltyError::InvalidRate))
    );
    assert_eq!(
        s.client.try_enroll(&s.merchant, &10, &UNIT),
        Err(Ok(LoyaltyError::AlreadyEnrolled))
    );
}

#[test]
fn pause_halts_accrual() {
    let s = setup();
    s.client.pause();
    assert_eq!(
        s.client
            .try_accrue(&s.merchant, &s.merchant, &s.customer, &UNIT),
        Err(Ok(LoyaltyError::Paused))
    );
}
