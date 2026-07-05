//! Composite-mandate pool types + pool constants. Pure data — no logic, no
//! storage. Depends only on `mandate` (for `SchedulePoint`).

use soroban_sdk::{contracttype, Address, BytesN, Vec};

use crate::mandate::SchedulePoint;

/// Stage-1 single-transaction capture ceiling. The most safety-relevant
/// constant in the composite layer: a pool larger than one transaction's
/// resource budget could be built but never cleared. Raise it only with a
/// measured `simulateTransaction` resource report checked into `security/`,
/// never by assumption.
pub const MAX_POOL_MEMBERS: u32 = 8;
/// Protocol time is unix seconds everywhere; ledgers appear only in TTL math.
/// Capture is valid only in `[clearing_deadline, clearing_deadline + CAPTURE_WINDOW_SECS]`.
pub const CAPTURE_WINDOW_SECS: u64 = 86_400;
/// `clearing_deadline + CAPTURE_WINDOW_SECS - now` must fit one TTL bump.
pub const MAX_POOL_HORIZON_SECS: u64 = 30 * 86_400;
pub const BPS_DENOM: i128 = 10_000;

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ClearingKind {
    ThresholdFloor,
    /// Reserved for Stage 2; `register_pool` rejects with `KindNotSupported`.
    SpendCeiling,
    /// Reserved for Stage 2; `register_pool` rejects with `KindNotSupported`.
    CapacityCeiling,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum PoolStatus {
    Open,
    Cleared,
    Aborted,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ClearingPool {
    /// Signs `register_pool`; holds NO later power — clearing is permissionless
    /// and deterministic, which is the whole no-skim guarantee.
    pub originator: Address,
    pub merchant: Address,
    pub asset: Address,
    pub kind: ClearingKind,
    /// Vendor minimum units; the pool fires only if aggregate qty reaches it.
    pub threshold_qty: u128,
    /// Vendor minimum order value, compared NET of fee to the merchant.
    pub threshold_value: u128,
    /// Floor on each committing child's worst_case (anti-dust squatting).
    pub min_child_value: u128,
    /// Unix seconds. Capture is a deadline auction: never before this instant.
    pub clearing_deadline: u64,
    /// Fee rate captured at `register_pool`; capture never reads a live rate.
    /// Always 0 in this deploy (the fee knob ships in its own pass); the field
    /// exists so that pass is not another ABI break.
    pub fee_bps_pinned: u32,
    pub status: PoolStatus,
    /// Live Committed members while Open; frozen at terminal status.
    pub member_count: u32,
}

/// The row `pool.rs` builds per committed child and feeds to `clearing::clear`.
/// Feeding plain values (not storage handles) is what keeps the clearing
/// function pure and makes simulate == capture a provable equality.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ChildView {
    pub mandate_id: BytesN<32>,
    pub schedule: Vec<SchedulePoint>,
    /// Decided once, before any price exists — see pool.rs eligibility.
    pub eligible: bool,
    pub worst_case: i128,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Allocation {
    pub mandate_id: BytesN<32>,
    pub qty: u128,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ClearOutcome {
    pub fires: bool,
    /// The single uniform price p*; 0 when `!fires`.
    pub clearing_price: i128,
    /// mandate_id order, qty > 0 only.
    pub allocations: Vec<Allocation>,
    pub total_qty: u128,
    pub gross_value: i128,
    pub total_fee: i128,
    /// `gross_value - total_fee`; the number compared to `threshold_value`.
    pub net_value: i128,
}
