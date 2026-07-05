//! Integration + §10 negative suite — runs in CI from commit one.
//! Each negative asserts the exact typed error (or host revert for auth); the
//! happy path asserts balances actually move through the SEP-41 token.

#![cfg(test)]

use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, BytesN, Env};

use crate::{Error, MandateRegistry, MandateRegistryClient, Status};

const NOW: u64 = 1_000;
const EXPIRY: u64 = 10_000;
const MAX: i128 = 50_000_000; // 5.00 USDC
const SPEND: i128 = 10_000_000; // 1.00 USDC
const FUNDED: i128 = 1_000_000_000;

struct World {
    env: Env,
    contract: Address,
    user: Address,
    agent: Address,
    merchant: Address,
    asset: Address,
    id: BytesN<32>,
}

fn setup() -> World {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);

    let contract = env.register(MandateRegistry, ());
    let user = Address::generate(&env);
    let agent = Address::generate(&env);
    let merchant = Address::generate(&env);

    let admin = Address::generate(&env);
    let asset = env.register_stellar_asset_contract_v2(admin).address();

    // Fund the user and approve the registry as the SEP-41 spender (allowance).
    StellarAssetClient::new(&env, &asset).mint(&user, &FUNDED);
    TokenClient::new(&env, &asset).approve(&user, &contract, &FUNDED, &100_000);

    let id = BytesN::from_array(&env, &[1u8; 32]);
    World {
        env,
        contract,
        user,
        agent,
        merchant,
        asset,
        id,
    }
}

impl World {
    fn client(&self) -> MandateRegistryClient<'_> {
        MandateRegistryClient::new(&self.env, &self.contract)
    }
    fn register(&self) {
        self.client().register_mandate(
            &self.user,
            &self.agent,
            &self.merchant,
            &self.asset,
            &MAX,
            &EXPIRY,
            &self.id,
            &None,
            &soroban_sdk::Vec::new(&self.env),
        );
    }
    fn balance(&self, who: &Address) -> i128 {
        TokenClient::new(&self.env, &self.asset).balance(who)
    }
}

// ── happy path — every method end to end ────────────────────────────────────

#[test]
fn happy_path_runs_every_method() {
    let w = setup();
    let c = w.client();

    // register
    let returned = c.register_mandate(
        &w.user,
        &w.agent,
        &w.merchant,
        &w.asset,
        &MAX,
        &EXPIRY,
        &w.id,
        &None,
        &soroban_sdk::Vec::new(&w.env),
    );
    assert_eq!(returned, w.id);

    // get_mandate
    let m = c.get_mandate(&w.id);
    assert_eq!(m.spent, 0);
    assert_eq!(m.max_amount, MAX);
    assert_eq!(m.seq, 0);

    // validate_mandate (read-only preflight)
    c.validate_mandate(&w.id, &SPEND, &w.merchant);

    // execute_payment — funds actually move (seq starts at 0)
    c.execute_payment(&w.id, &SPEND, &0);
    assert_eq!(w.balance(&w.merchant), SPEND);
    assert_eq!(w.balance(&w.user), FUNDED - SPEND);
    assert_eq!(c.get_mandate(&w.id).spent, SPEND);
    assert_eq!(c.get_mandate(&w.id).seq, 1);

    // revoke_mandate (seq is now 1)
    c.revoke_mandate(&w.id);
    assert_eq!(
        c.try_execute_payment(&w.id, &SPEND, &1),
        Err(Ok(Error::MandateRevoked))
    );
}

#[test]
fn property_spent_equals_transferred() {
    let w = setup();
    let c = w.client();
    w.register();
    c.execute_payment(&w.id, &SPEND, &0);
    c.execute_payment(&w.id, &SPEND, &1);
    assert_eq!(c.get_mandate(&w.id).spent, 2 * SPEND);
    assert_eq!(w.balance(&w.merchant), 2 * SPEND);
}

// ── §10 negative suite ──────────────────────────────────────────────────────

#[test]
fn duplicate_register_rejected() {
    let w = setup();
    w.register();
    assert_eq!(
        w.client().try_register_mandate(
            &w.user,
            &w.agent,
            &w.merchant,
            &w.asset,
            &MAX,
            &EXPIRY,
            &w.id,
            &None,
            &soroban_sdk::Vec::new(&w.env),
        ),
        Err(Ok(Error::AlreadyExists))
    );
}

#[test]
fn unknown_mandate_not_found() {
    let w = setup();
    let unknown = BytesN::from_array(&w.env, &[9u8; 32]);
    assert_eq!(
        w.client().try_get_mandate(&unknown),
        Err(Ok(Error::NotFound))
    );
    assert_eq!(
        w.client().try_execute_payment(&unknown, &SPEND, &0),
        Err(Ok(Error::NotFound))
    );
}

#[test]
fn overspend_single_rejected() {
    let w = setup();
    w.register();
    assert_eq!(
        w.client().try_execute_payment(&w.id, &(MAX + 1), &0),
        Err(Ok(Error::BudgetExceeded))
    );
    assert_eq!(w.balance(&w.merchant), 0);
}

#[test]
fn overspend_cumulative_rejected() {
    let w = setup();
    let c = w.client();
    w.register();
    c.execute_payment(&w.id, &(MAX - SPEND), &0); // ok
    assert_eq!(
        c.try_execute_payment(&w.id, &(SPEND + 1), &1),
        Err(Ok(Error::BudgetExceeded))
    );
    assert_eq!(w.balance(&w.merchant), MAX - SPEND);
}

#[test]
fn expired_mandate_rejected() {
    let w = setup();
    w.register();
    w.env.ledger().set_timestamp(EXPIRY + 1);
    assert_eq!(
        w.client().try_execute_payment(&w.id, &SPEND, &0),
        Err(Ok(Error::MandateExpired))
    );
    assert_eq!(w.balance(&w.merchant), 0);
}

#[test]
fn revoked_mandate_rejected() {
    let w = setup();
    w.register();
    w.client().revoke_mandate(&w.id);
    assert_eq!(
        w.client().try_execute_payment(&w.id, &SPEND, &0),
        Err(Ok(Error::MandateRevoked))
    );
}

#[test]
fn out_of_scope_merchant_rejected() {
    let w = setup();
    w.register();
    let attacker = Address::generate(&w.env);
    assert_eq!(
        w.client().try_validate_mandate(&w.id, &SPEND, &attacker),
        Err(Ok(Error::MerchantOutOfScope))
    );
}

#[test]
fn zero_amount_rejected() {
    let w = setup();
    w.register();
    assert_eq!(
        w.client().try_execute_payment(&w.id, &0, &0),
        Err(Ok(Error::InvalidAmount))
    );
}

#[test]
fn register_with_past_expiry_rejected() {
    let w = setup();
    assert_eq!(
        w.client().try_register_mandate(
            &w.user,
            &w.agent,
            &w.merchant,
            &w.asset,
            &MAX,
            &(NOW - 1),
            &w.id,
            &None,
            &soroban_sdk::Vec::new(&w.env),
        ),
        Err(Ok(Error::MandateExpired))
    );
}

// ── replay / sequence (§4.4) ────────────────────────────────────────────────

#[test]
fn replay_stale_seq_rejected() {
    let w = setup();
    let c = w.client();
    w.register();
    c.execute_payment(&w.id, &SPEND, &0); // consumes seq 0, advances to 1
                                          // Re-submitting the same (now stale) seq is a replay → rejected.
    assert_eq!(
        c.try_execute_payment(&w.id, &SPEND, &0),
        Err(Ok(Error::BadSequence))
    );
    assert_eq!(w.balance(&w.merchant), SPEND); // moved exactly once
}

#[test]
fn out_of_order_seq_rejected() {
    let w = setup();
    w.register();
    // Current seq is 0; a future/out-of-order seq is rejected.
    assert_eq!(
        w.client().try_execute_payment(&w.id, &SPEND, &7),
        Err(Ok(Error::BadSequence))
    );
    assert_eq!(w.balance(&w.merchant), 0);
}

// ── auth suite (the security-central cases) ─────────────────────────────────
// These do NOT mock_all_auths for the call under test, so a missing/forged
// authorization makes the call revert at the host layer (Err(Err(_))).

#[test]
fn register_requires_user_auth() {
    let env = Env::default();
    env.ledger().set_timestamp(NOW);
    let contract = env.register(MandateRegistry, ());
    let client = MandateRegistryClient::new(&env, &contract);
    let user = Address::generate(&env);
    let agent = Address::generate(&env);
    let merchant = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let id = BytesN::from_array(&env, &[2u8; 32]);

    // No auths mocked → user.require_auth() must fail.
    let r = client.try_register_mandate(
        &user,
        &agent,
        &merchant,
        &asset,
        &MAX,
        &EXPIRY,
        &id,
        &None,
        &soroban_sdk::Vec::new(&env),
    );
    assert!(r.is_err());
}

#[test]
fn execute_requires_agent_auth() {
    let w = setup();
    w.register();
    w.env.set_auths(&[]); // clear all mocked auths
    let r = w.client().try_execute_payment(&w.id, &SPEND, &0);
    assert!(r.is_err());
    assert_eq!(w.balance(&w.merchant), 0); // no funds moved without agent auth
}

#[test]
fn revoke_requires_user_auth() {
    let w = setup();
    w.register();
    w.env.set_auths(&[]);
    assert!(w.client().try_revoke_mandate(&w.id).is_err());
    assert_eq!(w.client().get_mandate(&w.id).status, Status::Active); // still active
}

// ── state-machine + defense-in-depth ────────────────────────────────────────

#[test]
fn exhausted_status_then_rejected() {
    let w = setup();
    let c = w.client();
    w.register();
    c.execute_payment(&w.id, &MAX, &0); // spends the whole budget
    assert_eq!(c.get_mandate(&w.id).status, Status::Exhausted);
    assert_eq!(
        c.try_execute_payment(&w.id, &1, &1),
        Err(Ok(Error::BudgetExceeded))
    );
}

#[test]
fn insufficient_allowance_blocks_payment() {
    let w = setup();
    // Within the contract's budget, but the SEP-41 allowance is the hard ceiling.
    TokenClient::new(&w.env, &w.asset).approve(&w.user, &w.contract, &(SPEND - 1), &100_000);
    w.register();
    assert!(w.client().try_execute_payment(&w.id, &SPEND, &0).is_err());
    assert_eq!(w.balance(&w.merchant), 0);
}
