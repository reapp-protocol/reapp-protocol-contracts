//! The `Mandate` type and its pure schedule helpers. No storage, no clock.
//!
//! A pooled mandate's `price_schedule` is a demand curve: entry
//! `(unit_price, max_qty)` means "at uniform clearing price <= unit_price, buy
//! up to max_qty units," so quantity falls as price rises. The schedule (plus
//! the SEP-41 allowance and revocability until the pool deadline) is the
//! user's ENTIRE authorization for the pool path — there is no per-capture
//! agent signature by design.

use soroban_sdk::{contracttype, Address, BytesN, Vec};

use crate::error::Error;

/// Schedule bounds. `MAX_UNIT_PRICE * MAX_QTY = 1e24` and the pool cap of 8
/// members keeps every clearing sum below 8e24, far under `i128::MAX` — the
/// three caps are the overflow-freedom argument and move together or not at
/// all (see pooltypes::MAX_POOL_MEMBERS).
pub const MAX_SCHEDULE_POINTS: u32 = 8;
pub const MAX_UNIT_PRICE: i128 = 1_000_000_000_000_000; // 1e15 stroops
pub const MAX_QTY: u128 = 1_000_000_000; // 1e9 units

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SchedulePoint {
    /// Strictly ascending across the schedule; each in (0, MAX_UNIT_PRICE].
    pub unit_price: i128,
    /// Strictly descending across the schedule; each in (0, MAX_QTY].
    pub max_qty: u128,
}

/// Pool linkage lifecycle, orthogonal to `Status`. `Unlinked` and `Released`
/// children may spend on the solo path (their own limits still apply);
/// `Committed` and `Captured` may not (`MandatePooled`).
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum PoolState {
    Unlinked,
    Committed,
    Captured,
    Released,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Mandate {
    /// Signer of the AP2 IntentMandate; grants the SEP-41 allowance.
    pub user: Address,
    /// The ONLY principal permitted to call `execute_payment`.
    pub agent: Address,
    /// MVP: single allowed payee (scope). T1: `Vec<Address>` or scope-hash.
    pub merchant: Address,
    /// SEP-41 / SAC contract id (USDC on testnet).
    pub asset: Address,
    /// Total budget authorized by the mandate.
    pub max_amount: i128,
    /// Cumulative consumed; invariant: `0 <= spent <= max_amount`.
    pub spent: i128,
    /// Ledger close timestamp (seconds) after which the mandate is dead.
    pub expiry: u64,
    /// Monotonic payment counter (mandate-level trace / replay guard).
    pub seq: u32,
    pub status: Status,
    /// Hash binding to the off-chain AP2 IntentMandate VC; also the storage key.
    pub vc_hash: BytesN<32>,
    /// `None` == standalone: exactly the pre-composite behavior.
    pub pool_id: Option<BytesN<32>>,
    /// The demand curve; empty when standalone.
    pub price_schedule: Vec<SchedulePoint>,
    pub pool_state: PoolState,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Active,
    Revoked,
    Exhausted,
}

/// Register-time gate. Strict ascending price + strict descending qty rejects
/// dominated/overlapping entries at the source, which is what collapses the
/// "3 at $5 OR 1 at $10" disjunction into a function evaluation at clear time.
pub fn validate_schedule(schedule: &Vec<SchedulePoint>) -> Result<(), Error> {
    let len = schedule.len();
    if len == 0 || len > MAX_SCHEDULE_POINTS {
        return Err(Error::ScheduleInvalid);
    }
    let mut prev_price: i128 = 0;
    let mut prev_qty: u128 = u128::MAX;
    for point in schedule.iter() {
        if point.unit_price <= prev_price || point.unit_price > MAX_UNIT_PRICE {
            return Err(Error::ScheduleInvalid);
        }
        if point.max_qty == 0 || point.max_qty > MAX_QTY || point.max_qty >= prev_qty {
            return Err(Error::ScheduleInvalid);
        }
        prev_price = point.unit_price;
        prev_qty = point.max_qty;
    }
    Ok(())
}

/// Quantity demanded at uniform price `p`: the `max_qty` of the FIRST entry
/// (lowest price) with `unit_price >= p`; 0 if `p` exceeds every entry.
/// `[(5,3),(10,1)]`: demand(5)=3, demand(7)=1, demand(10)=1, demand(11)=0.
pub fn demand(schedule: &Vec<SchedulePoint>, p: i128) -> u128 {
    for point in schedule.iter() {
        if point.unit_price >= p {
            return point.max_qty;
        }
    }
    0
}

/// The largest leg any clearing price can produce for this schedule:
/// max over entries of `unit_price * max_qty` (each entry is its own demand at
/// its own price). Bounded by MAX_UNIT_PRICE * MAX_QTY, so it always fits i128.
pub fn worst_case(schedule: &Vec<SchedulePoint>) -> i128 {
    let mut best: i128 = 0;
    for point in schedule.iter() {
        let leg = point.unit_price * point.max_qty as i128;
        if leg > best {
            best = leg;
        }
    }
    best
}
