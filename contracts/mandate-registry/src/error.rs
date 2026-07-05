//! Typed errors. Leaf module — depends on nothing.
//!
//! NOTE on authorization: unauthorized callers are rejected by Soroban's host
//! `require_auth` (a transaction revert), which is the correct Soroban pattern
//! and does NOT surface a contract-typed error. So there is no `NotAuthorized`
//! variant — the test suite asserts the host-level revert instead.
//!
//! Reserved slots (kept free so future passes are not ABI breaks):
//!   3  — was NotAuthorized (host-enforced, see above)
//!   10 — Paused (admin/pause pass)
//!   23 — FeeTooHigh, 24 — FeeRecipientNotSet (fee-knob pass)

use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Error {
    AlreadyExists = 1,
    NotFound = 2,
    MandateExpired = 4,
    MandateRevoked = 5,
    BudgetExceeded = 6,
    MerchantOutOfScope = 7,
    BadSequence = 8,
    InvalidAmount = 9,
    // ── composite layer ──────────────────────────────────────────────────
    PoolNotFound = 11,
    PoolNotOpen = 12,
    ScheduleInvalid = 13,
    PoolMerchantMismatch = 14,
    PoolAssetMismatch = 15,
    DeadlinePassed = 16,
    /// Reserved for outcome-style reporting; the abort branch is a success
    /// (state flip + event), not an error.
    ThresholdNotMet = 17,
    PoolFull = 18,
    BadPoolState = 19,
    MandatePooled = 20,
    InsufficientFunds = 21,
    KindNotSupported = 22,
    NotPooled = 25,
    ExpiryBeforeDeadline = 26,
    BelowMinChild = 27,
    DuplicateMember = 28,
    DeadlineNotReached = 29,
    DeadlineTooFar = 30,
    MemberStillEligible = 31,
}
