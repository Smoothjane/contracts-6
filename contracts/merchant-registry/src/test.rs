extern crate std;

use super::*;
use soroban_sdk::{testutils::Address as _, vec, Env, String};

struct Setup<'a> {
    env: Env,
    client: MerchantRegistryClient<'a>,
    owner: Address,
    settlement: Address,
    usdc: Address,
}

fn setup() -> Setup<'static> {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(MerchantRegistry, ());
    let client = MerchantRegistryClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    Setup {
        owner: Address::generate(&env),
        settlement: Address::generate(&env),
        usdc: Address::generate(&env),
        client,
        env,
    }
}

fn register(s: &Setup) {
    s.client.register(
        &s.owner,
        &String::from_str(&s.env, "Kofi's Coffee"),
        &s.settlement,
        &vec![&s.env, s.usdc.clone()],
    );
}

#[test]
fn registration_starts_pending_and_cannot_accept_payments() {
    let s = setup();
    register(&s);

    let m = s.client.get_merchant(&s.owner);
    assert_eq!(m.status, MerchantStatus::Pending);
    assert_eq!(s.client.merchant_count(), 1);
    assert!(!s.client.is_active(&s.owner));
    assert!(!s.client.accepts(&s.owner, &s.usdc));
}

#[test]
fn approved_merchant_accepts_only_listed_assets() {
    let s = setup();
    register(&s);
    s.client.approve(&s.owner);

    assert!(s.client.is_active(&s.owner));
    assert!(s.client.accepts(&s.owner, &s.usdc));
    assert!(!s.client.accepts(&s.owner, &Address::generate(&s.env)));
    assert_eq!(s.client.settlement_of(&s.owner), s.settlement);
}

#[test]
fn suspension_blocks_settlement_until_reinstated() {
    let s = setup();
    register(&s);
    s.client.approve(&s.owner);
    s.client.suspend(&s.owner);

    assert!(!s.client.accepts(&s.owner, &s.usdc));
    assert_eq!(
        s.client.try_settlement_of(&s.owner),
        Err(Ok(RegistryError::InvalidStatusTransition))
    );

    s.client.reinstate(&s.owner);
    assert!(s.client.is_active(&s.owner));
}

#[test]
fn status_transitions_are_enforced() {
    let s = setup();
    register(&s);
    // Pending merchants cannot be suspended or reinstated.
    assert_eq!(
        s.client.try_suspend(&s.owner),
        Err(Ok(RegistryError::InvalidStatusTransition))
    );
    assert_eq!(
        s.client.try_reinstate(&s.owner),
        Err(Ok(RegistryError::InvalidStatusTransition))
    );
}

#[test]
fn duplicate_registration_is_rejected() {
    let s = setup();
    register(&s);
    let again = s.client.try_register(
        &s.owner,
        &String::from_str(&s.env, "Again"),
        &s.settlement,
        &vec![&s.env],
    );
    assert_eq!(again, Err(Ok(RegistryError::AlreadyRegistered)));
}

#[test]
fn asset_list_validation() {
    let s = setup();
    let dup = s.client.try_register(
        &s.owner,
        &String::from_str(&s.env, "Dupes"),
        &s.settlement,
        &vec![&s.env, s.usdc.clone(), s.usdc.clone()],
    );
    assert_eq!(dup, Err(Ok(RegistryError::DuplicateAsset)));

    let mut many = vec![&s.env];
    for _ in 0..=MAX_ACCEPTED_ASSETS {
        many.push_back(Address::generate(&s.env));
    }
    let too_many = s.client.try_register(
        &s.owner,
        &String::from_str(&s.env, "Many"),
        &s.settlement,
        &many,
    );
    assert_eq!(too_many, Err(Ok(RegistryError::TooManyAssets)));
}

#[test]
fn empty_name_is_rejected() {
    let s = setup();
    let res = s.client.try_register(
        &s.owner,
        &String::from_str(&s.env, ""),
        &s.settlement,
        &vec![&s.env],
    );
    assert_eq!(res, Err(Ok(RegistryError::InvalidName)));
}

#[test]
fn profile_update_changes_settlement_and_assets() {
    let s = setup();
    register(&s);
    s.client.approve(&s.owner);

    let new_settlement = Address::generate(&s.env);
    let xlm = Address::generate(&s.env);
    s.client
        .update_profile(&s.owner, &new_settlement, &vec![&s.env, xlm.clone()]);

    assert_eq!(s.client.settlement_of(&s.owner), new_settlement);
    assert!(s.client.accepts(&s.owner, &xlm));
    assert!(!s.client.accepts(&s.owner, &s.usdc));
}

#[test]
fn pause_blocks_registration() {
    let s = setup();
    s.client.pause();
    assert!(s.client.is_paused());
    let res = s.client.try_register(
        &s.owner,
        &String::from_str(&s.env, "Paused"),
        &s.settlement,
        &vec![&s.env],
    );
    assert_eq!(res, Err(Ok(RegistryError::Paused)));
    s.client.unpause();
    register(&s);
}

#[test]
fn initialize_twice_fails() {
    let s = setup();
    assert_eq!(
        s.client.try_initialize(&Address::generate(&s.env)),
        Err(Ok(RegistryError::AlreadyInitialized))
    );
}
