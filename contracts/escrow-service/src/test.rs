extern crate std;

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    Env,
};

pub(crate) const DAY: u64 = 24 * 60 * 60;

pub(crate) struct Setup<'a> {
    pub env: Env,
    pub client: EscrowServiceClient<'a>,
    pub token: token::Client<'a>,
    pub asset: Address,
    pub buyer: Address,
    pub seller: Address,
    pub arbiter: Address,
}

pub(crate) fn setup() -> Setup<'static> {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let id = env.register(EscrowService, ());
    let client = EscrowServiceClient::new(&env, &id);
    client.initialize(&Address::generate(&env));

    let asset = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let buyer = Address::generate(&env);
    StellarAssetClient::new(&env, &asset).mint(&buyer, &10_000);

    Setup {
        token: token::Client::new(&env, &asset),
        client,
        asset,
        buyer,
        seller: Address::generate(&env),
        arbiter: Address::generate(&env),
        env,
    }
}

pub(crate) fn open(s: &Setup, amount: i128) -> u64 {
    let now = s.env.ledger().timestamp();
    s.client.create(
        &s.buyer,
        &s.seller,
        &s.arbiter,
        &s.asset,
        &amount,
        &(now + 3 * DAY),
        &(2 * DAY),
    )
}

#[test]
fn create_moves_funds_into_escrow() {
    let s = setup();
    let id = open(&s, 1_000);
    assert_eq!(s.token.balance(&s.buyer), 9_000);
    assert_eq!(s.token.balance(&s.client.address), 1_000);
    let e = s.client.get_escrow(&id);
    assert_eq!(e.status, EscrowStatus::Funded);
    assert_eq!(e.remaining(), 1_000);
}

#[test]
fn buyer_release_pays_seller() {
    let s = setup();
    let id = open(&s, 1_000);
    s.client.release(&id);
    assert_eq!(s.token.balance(&s.seller), 1_000);
    assert_eq!(s.client.get_escrow(&id).status, EscrowStatus::Released);
    assert_eq!(
        s.client.try_release(&id),
        Err(Ok(EscrowError::InvalidStatus))
    );
}

#[test]
fn partial_releases_add_up() {
    let s = setup();
    let id = open(&s, 1_000);
    assert_eq!(s.client.release_partial(&id, &300), 700);
    assert_eq!(s.client.release_partial(&id, &700), 0);
    assert_eq!(s.token.balance(&s.seller), 1_000);
    assert_eq!(s.client.get_escrow(&id).status, EscrowStatus::Released);
}

#[test]
fn partial_release_cannot_exceed_remaining() {
    let s = setup();
    let id = open(&s, 1_000);
    s.client.release_partial(&id, &600);
    assert_eq!(
        s.client.try_release_partial(&id, &401),
        Err(Ok(EscrowError::InvalidAmount))
    );
}

#[test]
fn seller_claims_only_after_release_time() {
    let s = setup();
    let id = open(&s, 1_000);
    assert_eq!(s.client.try_claim(&id), Err(Ok(EscrowError::ReleaseNotDue)));
    s.env.ledger().with_mut(|l| l.timestamp += 3 * DAY);
    s.client.claim(&id);
    assert_eq!(s.token.balance(&s.seller), 1_000);
}

#[test]
fn seller_refund_returns_remainder_even_while_paused() {
    let s = setup();
    let id = open(&s, 1_000);
    s.client.release_partial(&id, &250);
    s.client.pause();
    s.client.refund(&id);
    assert_eq!(s.token.balance(&s.buyer), 9_750);
    assert_eq!(s.token.balance(&s.seller), 250);
    assert_eq!(s.client.get_escrow(&id).status, EscrowStatus::Refunded);
}

#[test]
fn pause_blocks_new_escrows() {
    let s = setup();
    s.client.pause();
    let now = s.env.ledger().timestamp();
    assert_eq!(
        s.client.try_create(
            &s.buyer,
            &s.seller,
            &s.arbiter,
            &s.asset,
            &1,
            &(now + DAY),
            &DAY
        ),
        Err(Ok(EscrowError::Paused))
    );
}

#[test]
fn creation_validation() {
    let s = setup();
    let now = s.env.ledger().timestamp();
    let c = &s.client;
    assert_eq!(
        c.try_create(
            &s.buyer,
            &s.seller,
            &s.arbiter,
            &s.asset,
            &0,
            &(now + DAY),
            &DAY
        ),
        Err(Ok(EscrowError::InvalidAmount))
    );
    assert_eq!(
        c.try_create(
            &s.buyer,
            &s.buyer,
            &s.arbiter,
            &s.asset,
            &1,
            &(now + DAY),
            &DAY
        ),
        Err(Ok(EscrowError::InvalidParties))
    );
    assert_eq!(
        c.try_create(
            &s.buyer,
            &s.seller,
            &s.seller,
            &s.asset,
            &1,
            &(now + DAY),
            &DAY
        ),
        Err(Ok(EscrowError::InvalidParties))
    );
    assert_eq!(
        c.try_create(&s.buyer, &s.seller, &s.arbiter, &s.asset, &1, &now, &DAY),
        Err(Ok(EscrowError::InvalidSchedule))
    );
    assert_eq!(
        c.try_create(
            &s.buyer,
            &s.seller,
            &s.arbiter,
            &s.asset,
            &1,
            &(now + DAY),
            &60
        ),
        Err(Ok(EscrowError::InvalidSchedule))
    );
}
