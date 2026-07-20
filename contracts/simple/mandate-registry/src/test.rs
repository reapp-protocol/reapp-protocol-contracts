//! Integration + §10 negative suite — runs in CI from commit one.
//! Each negative asserts the exact typed error (or host revert for auth); the
//! happy path asserts balances actually move through the SEP-41 token.

#![cfg(test)]

use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{symbol_short, Address, Bytes, BytesN, Env, IntoVal};

use crate::{
    Error, MandateRegistry, MandateRegistryClient, PendingUpgrade, Status, UPGRADE_DELAY_SECONDS,
};

const NOW: u64 = 1_000;
const EXPIRY: u64 = 10_000;
const MAX: i128 = 50_000_000; // 5.00 USDC
const SPEND: i128 = 10_000_000; // 1.00 USDC
const FUNDED: i128 = 1_000_000_000;

// Protocol-21 `add(u64,u64)->u64` fixture from soroban-sdk 22.0.11. Keeping
// the small fixture as text makes the test portable without a generated binary
// or build-script dependency.
const REPLACEMENT_WASM_HEX: &str = "0061736d0100000001140460017e017e60027f7e0060027e7e017e600000020d020169013000000169015f0000030605010203030305030100100619037f01418080c0000b7f00418080c0000b7f00418080c0000b072f05066d656d6f72790200036164640003015f00060a5f5f646174615f656e6403010b5f5f686561705f6261736503020a8c02055d02017f017e024002402001a741ff0171220241c000460d00024020024106460d00420121034283908080800121010c020b20014208882101420021030c010b42002103200110808080800021010b20002001370308200020033703000b990101017f23808080800041206b2202248080808000200241106a20001082808080000240024020022802100d0020022903182100200220011082808080002002290300a70d00200020022903087c22012000540d0102400240200142ffffffffffffffff00560d00200142088642068421000c010b200110818080800021000b200241206a24808080800020000f0b00000b108480808000000b0900108580808000000b040000000b02000b004b0e636f6e7472616374737065637630000000000000000000000003616464000000000200000000000000016100000000000006000000000000000162000000000000060000000100000006001e11636f6e7472616374656e766d6574617630000000000000001500000000007b0e636f6e74726163746d65746176300000000000000005727376657200000000000006312e37342e3000000000000000000008727373646b7665720000003932312e302e312d707265766965772e312331313663333562633965303366346231623565363562356565383331616530663836616139326664000000";

fn replacement_wasm(env: &Env) -> Bytes {
    fn nibble(value: u8) -> u8 {
        match value {
            b'0'..=b'9' => value - b'0',
            b'a'..=b'f' => value - b'a' + 10,
            _ => panic!("invalid replacement WASM hex"),
        }
    }
    let encoded = REPLACEMENT_WASM_HEX.as_bytes();
    let mut wasm = Bytes::new(env);
    let mut index = 0;
    while index < encoded.len() {
        wasm.push_back((nibble(encoded[index]) << 4) | nibble(encoded[index + 1]));
        index += 2;
    }
    wasm
}

struct World {
    env: Env,
    contract: Address,
    admin: Address,
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

    let admin = Address::from_str(
        &env,
        "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF",
    );
    let contract = env.register(MandateRegistry, (admin.clone(),));
    let user = Address::generate(&env);
    let agent = Address::generate(&env);
    let merchant = Address::generate(&env);

    let asset_admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(asset_admin)
        .address();

    // Fund the user and approve the registry as the SEP-41 spender (allowance).
    StellarAssetClient::new(&env, &asset).mint(&user, &FUNDED);
    TokenClient::new(&env, &asset).approve(&user, &contract, &FUNDED, &100_000);

    let id = BytesN::from_array(&env, &[1u8; 32]);
    World {
        env,
        contract,
        admin,
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

// ── administration + emergency stop ───────────────────────────────────────

#[test]
fn constructor_sets_admin_and_unpaused_state() {
    let w = setup();
    assert_eq!(w.client().get_admin(), w.admin);
    assert!(!w.client().is_paused());
}

#[test]
fn admin_can_pause_and_unpause_idempotently() {
    let w = setup();
    let c = w.client();
    c.pause();
    c.pause();
    assert!(c.is_paused());
    c.unpause();
    c.unpause();
    assert!(!c.is_paused());
}

#[test]
fn pause_blocks_payment_without_changing_mandate_state() {
    let w = setup();
    let c = w.client();
    w.register();
    c.pause();

    assert_eq!(
        c.try_execute_payment(&w.id, &SPEND, &0),
        Err(Ok(Error::Paused))
    );
    assert_eq!(c.get_mandate(&w.id).spent, 0);
    assert_eq!(c.get_mandate(&w.id).seq, 0);
    assert_eq!(w.balance(&w.merchant), 0);

    c.unpause();
    c.execute_payment(&w.id, &SPEND, &0);
    assert_eq!(w.balance(&w.merchant), SPEND);
}

#[test]
fn registration_validation_reads_and_revocation_remain_available_while_paused() {
    let w = setup();
    let c = w.client();
    c.pause();

    w.register();
    c.validate_mandate(&w.id, &SPEND, &w.merchant);
    assert_eq!(c.get_mandate(&w.id).status, Status::Active);
    c.revoke_mandate(&w.id);
    assert_eq!(c.get_mandate(&w.id).status, Status::Revoked);
}

#[test]
fn admin_rotation_transfers_control() {
    let w = setup();
    let c = w.client();
    let new_admin = Address::generate(&w.env);
    c.set_admin(&new_admin);
    assert_eq!(c.get_admin(), new_admin);

    w.env.set_auths(&[]);
    assert!(c.try_pause().is_err());

    w.env.mock_all_auths();
    c.pause();
    assert!(c.is_paused());
}

#[test]
fn admin_methods_require_authorization() {
    let w = setup();
    let c = w.client();
    let replacement = Address::generate(&w.env);
    let wasm_hash = BytesN::from_array(&w.env, &[42u8; 32]);
    w.env.set_auths(&[]);

    assert!(c.try_pause().is_err());
    assert!(c.try_unpause().is_err());
    assert!(c.try_set_admin(&replacement).is_err());
    assert!(c.try_schedule_upgrade(&wasm_hash).is_err());
    assert!(c.try_cancel_upgrade().is_err());
    assert!(c.try_execute_upgrade().is_err());
    assert!(!c.is_paused());
    assert_eq!(c.get_admin(), w.admin);
}

#[test]
fn cancel_and_execute_require_authorization_with_ready_upgrade() {
    let w = setup();
    let c = w.client();
    w.register();
    let mandate_before = c.get_mandate(&w.id);
    let wasm_hash = w
        .env
        .deployer()
        .upload_contract_wasm(replacement_wasm(&w.env));
    let execute_after = c.schedule_upgrade(&wasm_hash);
    c.pause();
    w.env.ledger().set_timestamp(execute_after);
    let pending_before = c.get_pending_upgrade();

    w.env.set_auths(&[]);

    assert!(matches!(c.try_cancel_upgrade(), Err(Err(_))));
    assert_eq!(c.get_pending_upgrade(), pending_before);
    assert!(matches!(c.try_execute_upgrade(), Err(Err(_))));
    assert_eq!(c.get_pending_upgrade(), pending_before);
    assert!(c.is_paused());
    assert_eq!(c.get_admin(), w.admin);
    assert_eq!(c.get_mandate(&w.id), mandate_before);
}

#[test]
fn timelocked_upgrade_replaces_wasm_at_same_address_and_preserves_storage() {
    let w = setup();
    let c = w.client();
    w.register();
    c.execute_payment(&w.id, &SPEND, &0);
    let mandate_before = c.get_mandate(&w.id);
    let contract_before = w.contract.clone();

    let wasm_hash = w
        .env
        .deployer()
        .upload_contract_wasm(replacement_wasm(&w.env));
    let execute_after = c.schedule_upgrade(&wasm_hash);
    assert_eq!(execute_after, NOW + UPGRADE_DELAY_SECONDS);
    assert_eq!(
        c.get_pending_upgrade(),
        Some(PendingUpgrade {
            wasm_hash: wasm_hash.clone(),
            execute_after,
        })
    );

    w.env.ledger().set_timestamp(execute_after - 1);
    assert_eq!(c.try_execute_upgrade(), Err(Ok(Error::UpgradeNotReady)));
    w.env.ledger().set_timestamp(execute_after);
    assert_eq!(
        c.try_execute_upgrade(),
        Err(Ok(Error::UpgradeRequiresPause))
    );

    c.pause();
    c.execute_upgrade();

    let sum: u64 = w.env.invoke_contract(
        &contract_before,
        &symbol_short!("add"),
        (2_u64, 3_u64).into_val(&w.env),
    );
    assert_eq!(sum, 5);
    assert_eq!(w.contract, contract_before);

    let (admin, paused, pending, mandate_after) = w.env.as_contract(&w.contract, || {
        (
            crate::storage::get_admin(&w.env),
            crate::storage::is_paused(&w.env),
            crate::storage::get_pending_upgrade(&w.env),
            crate::storage::get_mandate(&w.env, w.id.clone()).unwrap(),
        )
    });
    assert_eq!(admin, w.admin);
    assert!(paused);
    assert_eq!(pending, None);
    assert_eq!(mandate_after, mandate_before);
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
            &w.id
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
            &w.id
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
    let contract = env.register(
        MandateRegistry,
        (Address::from_str(
            &env,
            "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF",
        ),),
    );
    let client = MandateRegistryClient::new(&env, &contract);
    let user = Address::generate(&env);
    let agent = Address::generate(&env);
    let merchant = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let id = BytesN::from_array(&env, &[2u8; 32]);

    // No auths mocked → user.require_auth() must fail.
    let r = client.try_register_mandate(&user, &agent, &merchant, &asset, &MAX, &EXPIRY, &id);
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
