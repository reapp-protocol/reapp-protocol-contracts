//! AP2 v0.2 pooled-extension types and cross-contract interface.
//!
//! Raw SD-JWT verification remains off-chain. These types are byte-identical
//! to the separate AP2 authorization extension's Soroban ABI.

use soroban_sdk::{contractclient, contracttype, Address, BytesN, Env};

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PoolParticipationAuthorization {
    pub version: u32,
    pub network_id: BytesN<32>,
    pub registry: Address,
    pub pool_id: BytesN<32>,
    pub mandate_id: BytesN<32>,
    pub agent: Address,
    pub merchant: Address,
    pub asset: Address,
    pub max_amount: i128,
    pub schedule_hash: BytesN<32>,
    pub open_checkout_evidence: BytesN<32>,
    pub closed_checkout_evidence: BytesN<32>,
    pub open_participation_evidence: BytesN<32>,
    pub closed_participation_evidence: BytesN<32>,
    pub nonce: BytesN<32>,
    pub verifier_key: BytesN<32>,
    pub not_before: u64,
    pub expires_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PoolCapture {
    pub amount: i128,
    pub expected_seq: u32,
    pub outcome_root: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Ap2PoolPolicy {
    pub extension: Address,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Ap2MandatePolicy {
    pub extension: Address,
    pub participation_id: BytesN<32>,
}

#[contractclient(name = "Ap2AuthorizationExtensionClient")]
#[allow(dead_code)]
pub trait Ap2AuthorizationExtension {
    fn register_pool_participation(
        env: Env,
        authorization: PoolParticipationAuthorization,
        signature: BytesN<64>,
    ) -> BytesN<32>;

    fn consume_pool(env: Env, participation_id: BytesN<32>, capture: PoolCapture);
}
