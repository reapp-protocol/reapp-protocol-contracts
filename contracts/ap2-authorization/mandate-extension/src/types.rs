//! Typed inputs shared with merchant-side authorization signers.

use soroban_sdk::{contracttype, Address, BytesN};

pub const AUTHORIZATION_VERSION: u32 = 1;

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum CaptureKind {
    Simple,
    CompositeSolo,
}

/// Exact, transaction-time authorization for a solo payment.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureAuthorization {
    pub version: u32,
    pub network_id: BytesN<32>,
    pub registry: Address,
    pub kind: CaptureKind,
    pub mandate_id: BytesN<32>,
    pub agent: Address,
    pub merchant: Address,
    pub asset: Address,
    pub amount: i128,
    pub expected_seq: u32,
    pub open_checkout_evidence: BytesN<32>,
    pub closed_checkout_evidence: BytesN<32>,
    pub open_payment_evidence: BytesN<32>,
    pub closed_payment_evidence: BytesN<32>,
    pub nonce: BytesN<32>,
    pub verifier_key: BytesN<32>,
    pub not_before: u64,
    pub expires_at: u64,
}

/// Pre-deadline authorization for one deterministic pooled schedule.
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

/// Exact deterministic result supplied by Composite at capture time.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PoolCapture {
    pub amount: i128,
    pub expected_seq: u32,
    pub outcome_root: BytesN<32>,
}
