//! Typed errors. Leaf module — depends on nothing.
//!
//! NOTE on authorization: unauthorized callers are rejected by Soroban's host
//! `require_auth` (a transaction revert), which is the correct Soroban pattern
//! and does NOT surface a contract-typed error. So there is no `NotAuthorized`
//! variant — the test suite asserts the host-level revert instead. (Slot 3 is
//! intentionally left free to keep the remaining codes stable.)

use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Error {
    AlreadyExists = 1,
    NotFound = 2,
    // 3 = (reserved; was NotAuthorized — auth is host-enforced via require_auth)
    MandateExpired = 4,
    MandateRevoked = 5,
    BudgetExceeded = 6,
    MerchantOutOfScope = 7,
    BadSequence = 8,
    InvalidAmount = 9,
}
