//! Read-only mirrors of the two registries' `Mandate` encodings.
//!
//! The extension forwards only `(mandate_id, amount, expected_seq)` to a
//! registry, so the merchant, asset and agent a verifier signed into a
//! `CaptureAuthorization` would otherwise be evidence nobody checks. Reading
//! the mandate back lets the extension refuse a capture whose registry state
//! disagrees with the AP2 evidence it was handed.
//!
//! Simple and Composite encode `Mandate` differently (Composite adds the pool
//! fields), so each route needs its own decode. Field *names* are the ScMap
//! keys, so these mirrors must track the registries exactly. Two existing
//! tests hold them to it against the real contracts, not against these copies:
//! `unchanged_simple_registry_moves_funds_and_keeps_the_allowance` here, and
//! `released_ap2_child_uses_composite_solo_route` in the Composite suite.

use soroban_sdk::{contractclient, contracttype, Address, BytesN, Env, Vec};

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum MandateStatus {
    Active,
    Revoked,
    Exhausted,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum MandatePoolState {
    Unlinked,
    Committed,
    Captured,
    Released,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct MandateSchedulePoint {
    pub unit_price: i128,
    pub max_qty: u128,
}

/// Mirrors `contracts/simple/mandate-registry` `Mandate`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SimpleMandate {
    pub user: Address,
    pub agent: Address,
    pub merchant: Address,
    pub asset: Address,
    pub max_amount: i128,
    pub spent: i128,
    pub expiry: u64,
    pub seq: u32,
    pub status: MandateStatus,
    pub vc_hash: BytesN<32>,
}

/// Mirrors `contracts/composites/mandate-registry` `Mandate`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CompositeMandate {
    pub user: Address,
    pub agent: Address,
    pub merchant: Address,
    pub asset: Address,
    pub max_amount: i128,
    pub spent: i128,
    pub expiry: u64,
    pub seq: u32,
    pub status: MandateStatus,
    pub vc_hash: BytesN<32>,
    pub pool_id: Option<BytesN<32>>,
    pub price_schedule: Vec<MandateSchedulePoint>,
    pub pool_state: MandatePoolState,
}

#[contractclient(name = "SimpleRegistryClient")]
pub trait SimpleRegistry {
    fn execute_payment(env: Env, mandate_id: BytesN<32>, amount: i128, expected_seq: u32);
    fn get_mandate(env: Env, mandate_id: BytesN<32>) -> SimpleMandate;
}

#[contractclient(name = "CompositeRegistryClient")]
pub trait CompositeRegistry {
    fn execute_payment(env: Env, mandate_id: BytesN<32>, amount: i128, expected_seq: u32);
    fn get_mandate(env: Env, mandate_id: BytesN<32>) -> CompositeMandate;
}
