//! Pool lifecycle: register, commit, evict, clear, simulate. Owns the capture
//! (the composite money path) and `build_child_views` — the ONE function that
//! turns stored + live-token state into the clearing function's input, called
//! identically by `clear_pool` and `simulate_clear` (the no-discretion
//! equality). Depends on `storage`, `clearing`, `events`, types — never on
//! `registry`/`payment`.
//!
//! Authorization model: `register_pool` is originator-signed and is the last
//! signature the pool path ever requires from anyone but the users. Commit,
//! evict, clear and simulate are permissionless: every check is objective
//! on-chain state, the users authorized terms + schedule + allowance at
//! registration, and a commit stays revocable until the deadline. Requiring
//! per-capture signatures would hand every member a free last-second veto.

use soroban_sdk::token::TokenClient;
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{Address, Bytes, BytesN, Env, IntoVal, Symbol, Vec};

use crate::error::Error;
use crate::mandate::{worst_case, Mandate, PoolState, Status};
use crate::pooltypes::{
    ChildView, ClearOutcome, ClearingKind, ClearingPool, PoolStatus, CAPTURE_WINDOW_SECS,
    MAX_POOL_HORIZON_SECS, MAX_POOL_MEMBERS,
};
use crate::{clearing, events, storage};

use crate::mandate::{MAX_QTY, MAX_UNIT_PRICE};

/// Largest value any pool can clear at: 8 members, each at the schedule caps.
/// A threshold above this can never fire, so registration rejects it (and the
/// bound is what makes the u128 → i128 threshold cast in `clearing` safe).
fn max_pool_value() -> u128 {
    (MAX_POOL_MEMBERS as u128) * (MAX_UNIT_PRICE as u128) * MAX_QTY
}

/// Register a clearing pool. The pool id is derived in-contract from the terms
/// (sha256 of their XDR), so the id COMMITS to the terms — front-running an id
/// with different terms is impossible; `nonce` distinguishes identical-term
/// pools. Authorized by the originator.
#[allow(clippy::too_many_arguments)]
pub fn register_pool(
    env: &Env,
    originator: Address,
    merchant: Address,
    asset: Address,
    kind: ClearingKind,
    threshold_qty: u128,
    threshold_value: u128,
    min_child_value: u128,
    clearing_deadline: u64,
    nonce: BytesN<32>,
) -> Result<BytesN<32>, Error> {
    originator.require_auth();

    if kind != ClearingKind::ThresholdFloor {
        return Err(Error::KindNotSupported);
    }
    if threshold_qty == 0 && threshold_value == 0 {
        return Err(Error::InvalidAmount);
    }
    // Reject thresholds no clearable pool could ever satisfy.
    if threshold_qty > (MAX_POOL_MEMBERS as u128) * MAX_QTY
        || threshold_value > max_pool_value()
        || min_child_value > (MAX_UNIT_PRICE as u128) * MAX_QTY
    {
        return Err(Error::InvalidAmount);
    }
    let now = env.ledger().timestamp();
    if now >= clearing_deadline {
        return Err(Error::DeadlinePassed);
    }
    // Checked: an absurd deadline is the typed error, never an overflow panic.
    let horizon = clearing_deadline
        .checked_add(CAPTURE_WINDOW_SECS)
        .and_then(|end| end.checked_sub(now))
        .ok_or(Error::DeadlineTooFar)?;
    if horizon > MAX_POOL_HORIZON_SECS {
        return Err(Error::DeadlineTooFar);
    }

    let payload = (
        originator.clone(),
        merchant.clone(),
        asset.clone(),
        kind.clone(),
        threshold_qty,
        threshold_value,
        min_child_value,
        clearing_deadline,
        nonce,
    );
    let pool_id: BytesN<32> = env.crypto().sha256(&payload.to_xdr(env)).into();
    if storage::has_pool(env, &pool_id) {
        return Err(Error::AlreadyExists);
    }

    let pool = ClearingPool {
        originator: originator.clone(),
        merchant: merchant.clone(),
        asset: asset.clone(),
        kind,
        threshold_qty,
        threshold_value,
        min_child_value,
        clearing_deadline,
        // The fee knob ships in its own pass; pools registered by this build
        // pin rate 0 forever (pinning at registration is the rate-front-run fix).
        fee_bps_pinned: 0,
        status: PoolStatus::Open,
        member_count: 0,
    };
    storage::set_pool(env, &pool_id, &pool);
    storage::set_pool_members(env, &pool_id, &Vec::new(env));
    storage::bump_pool_horizon(env, &pool_id, horizon);
    events::pool_registered(
        env,
        &pool_id,
        &originator,
        &merchant,
        &asset,
        threshold_qty,
        threshold_value,
        clearing_deadline,
    );
    Ok(pool_id)
}

/// Link a registered pooled mandate into its pool as a Committed member.
/// Permissionless: every check below is objective on-chain state, and the
/// commit stays revocable (via `revoke_mandate`) until the deadline. The fund
/// check here is a courtesy preflight, NOT a reservation — SEP-41 allowance is
/// fungible and the user can move it; correctness comes entirely from the
/// capture-time eligibility filter.
pub fn commit_child(env: &Env, mandate_id: BytesN<32>) -> Result<(), Error> {
    let mut mandate = storage::get_mandate(env, mandate_id.clone())?;
    let pool_id = mandate.pool_id.clone().ok_or(Error::NotPooled)?;

    match mandate.status {
        Status::Revoked => return Err(Error::MandateRevoked),
        Status::Exhausted => return Err(Error::BudgetExceeded),
        Status::Active => {}
    }
    let now = env.ledger().timestamp();
    if now >= mandate.expiry {
        return Err(Error::MandateExpired);
    }
    if mandate.pool_state != PoolState::Unlinked {
        return Err(Error::BadPoolState);
    }

    let mut pool = storage::get_pool(env, pool_id.clone())?;
    if pool.status != PoolStatus::Open {
        return Err(Error::PoolNotOpen);
    }
    if now >= pool.clearing_deadline {
        return Err(Error::DeadlinePassed);
    }
    if pool.member_count >= MAX_POOL_MEMBERS {
        return Err(Error::PoolFull);
    }

    // Self-sybil dedup: at most one Committed child per user per pool, so one
    // allowance cannot be double-counted toward the threshold.
    let mut members = storage::get_pool_members(env, &pool_id);
    for member_id in members.iter() {
        let member = storage::get_mandate(env, member_id.clone())?;
        if member.pool_state == PoolState::Committed && member.user == mandate.user {
            return Err(Error::DuplicateMember);
        }
    }

    // Courtesy preflight (same terms as the capture-time eligibility filter).
    let wc = worst_case(&mandate.price_schedule);
    if mandate.max_amount - mandate.spent < wc {
        return Err(Error::InsufficientFunds);
    }
    let token = TokenClient::new(env, &pool.asset);
    let contract = env.current_contract_address();
    if token.allowance(&mandate.user, &contract) < wc || token.balance(&mandate.user) < wc {
        return Err(Error::InsufficientFunds);
    }
    if !trustline_authorized(env, &pool.asset, &mandate.user) {
        return Err(Error::InsufficientFunds);
    }

    mandate.pool_state = PoolState::Committed;
    storage::set_mandate(env, &mandate_id, &mandate);
    members.push_back(mandate_id.clone());
    storage::set_pool_members(env, &pool_id, &members);
    pool.member_count += 1;
    storage::set_pool(env, &pool_id, &pool);

    let horizon = pool.clearing_deadline + CAPTURE_WINDOW_SECS - now;
    storage::bump_pool_horizon(env, &pool_id, horizon);
    storage::bump_mandate_horizon(env, &mandate_id, horizon);
    events::child_committed(env, &pool_id, &mandate_id, wc);
    Ok(())
}

/// Remove an objectively-ineligible member and free its slot. Permissionless
/// garbage collection: it can NEVER evict a still-eligible member, so it grants
/// no discretion — it only reclaims the scarce MAX_POOL_MEMBERS slots that
/// dust/pulled-allowance squatters would otherwise hold. Not needed for
/// correctness (the clearing filter already excludes ineligible members).
pub fn evict_child(env: &Env, pool_id: BytesN<32>, mandate_id: BytesN<32>) -> Result<(), Error> {
    let mut pool = storage::get_pool(env, pool_id.clone())?;
    if pool.status != PoolStatus::Open {
        return Err(Error::PoolNotOpen);
    }
    let mut mandate = storage::get_mandate(env, mandate_id.clone())?;
    if mandate.pool_id != Some(pool_id.clone()) || mandate.pool_state != PoolState::Committed {
        return Err(Error::BadPoolState);
    }
    if is_eligible(env, &pool, &mandate) {
        return Err(Error::MemberStillEligible);
    }

    let mut members = storage::get_pool_members(env, &pool_id);
    if let Some(index) = members.first_index_of(mandate_id.clone()) {
        members.remove(index);
    }
    storage::set_pool_members(env, &pool_id, &members);
    pool.member_count = pool.member_count.saturating_sub(1);
    storage::set_pool(env, &pool_id, &pool);
    mandate.pool_state = PoolState::Released;
    storage::set_mandate(env, &mandate_id, &mandate);
    let horizon =
        (pool.clearing_deadline + CAPTURE_WINDOW_SECS).saturating_sub(env.ledger().timestamp());
    storage::bump_pool_horizon(env, &pool_id, horizon);
    storage::bump_mandate_horizon(env, &mandate_id, horizon);
    events::child_released(env, &pool_id, &mandate_id);
    Ok(())
}

/// The composite money path: the deadline auction's close. Callable by anyone,
/// never before the deadline (pre-deadline feasibility grants no one a timing
/// option — p* is a function of the committed set at close). Within the
/// capture window: recompute the canonical outcome and, if it fires, settle
/// every leg in this one transaction (all-or-nothing over the fired set).
/// Past the window, or if the predicate is unmet at close: abort, releasing
/// every committed child. Idempotent via the Open-status guard.
pub fn clear_pool(env: &Env, pool_id: BytesN<32>) -> Result<(), Error> {
    let mut pool = storage::get_pool(env, pool_id.clone())?;
    if pool.status != PoolStatus::Open {
        return Err(Error::PoolNotOpen);
    }
    let now = env.ledger().timestamp();
    if now < pool.clearing_deadline {
        return Err(Error::DeadlineNotReached);
    }

    let members = storage::get_pool_members(env, &pool_id);
    let outcome = if now <= pool.clearing_deadline + CAPTURE_WINDOW_SECS {
        let views = build_child_views(env, &pool, &members);
        clearing::clear(env, &pool, &views)
    } else {
        // A met-but-never-cleared pool cannot be captured months later
        // against unwatching participants: past the window, abort only.
        clearing::no_fire(env)
    };

    if !outcome.fires {
        pool.status = PoolStatus::Aborted;
        storage::set_pool(env, &pool_id, &pool);
        for member_id in members.iter() {
            let mut member = storage::get_mandate(env, member_id.clone())?;
            if member.pool_state == PoolState::Committed {
                member.pool_state = PoolState::Released;
                storage::set_mandate(env, &member_id, &member);
                events::child_released(env, &pool_id, &member_id);
            }
        }
        storage::bump_pool_horizon(env, &pool_id, 0); // terminal: standard extension floor
        events::pool_aborted(env, &pool_id);
        return Ok(());
    }

    // Checks-effects-interactions: persist ALL state before any transfer, so a
    // reentrant clear_pool during a transfer callback finds PoolNotOpen.
    pool.status = PoolStatus::Cleared;
    storage::set_pool(env, &pool_id, &pool);
    for member_id in members.iter() {
        let mut member = storage::get_mandate(env, member_id.clone())?;
        if member.pool_state != PoolState::Committed {
            continue;
        }
        let qty = allocation_qty(&outcome, &member_id);
        if qty == 0 {
            // Excluded (couldn't pay / expired) or priced out at p*: released,
            // never charged, solo path reopens within its own limits.
            member.pool_state = PoolState::Released;
            storage::set_mandate(env, &member_id, &member);
            events::child_released(env, &pool_id, &member_id);
            continue;
        }
        let leg = outcome.clearing_price * qty as i128;
        member.spent += leg;
        if member.spent > member.max_amount {
            // Unreachable: eligibility requires max_amount - spent >= worst_case
            // and leg <= worst_case. Guarded anyway — revert, never overcharge.
            return Err(Error::BudgetExceeded);
        }
        member.seq += 1;
        member.pool_state = PoolState::Captured;
        if member.spent == member.max_amount {
            member.status = Status::Exhausted;
        }
        storage::set_mandate(env, &member_id, &member);
    }

    // Interactions last, in allocation (mandate_id) order. fee_bps_pinned is 0
    // for every pool this build can register, so the merchant leg is the whole
    // leg; the split stays exact when the fee pass lands (floored fee).
    let token = TokenClient::new(env, &pool.asset);
    let contract = env.current_contract_address();
    for allocation in outcome.allocations.iter() {
        let member = storage::get_mandate(env, allocation.mandate_id.clone())?;
        let leg = outcome.clearing_price * allocation.qty as i128;
        let fee = leg * pool.fee_bps_pinned as i128 / crate::pooltypes::BPS_DENOM;
        token.transfer_from(&contract, &member.user, &pool.merchant, &(leg - fee));
    }

    storage::bump_pool_horizon(env, &pool_id, 0); // terminal: standard extension floor
    let root = allocation_root(env, &pool_id, &outcome);
    events::pool_cleared(
        env,
        &pool_id,
        outcome.clearing_price,
        &root,
        outcome.net_value,
        outcome.total_fee,
    );
    Ok(())
}

/// Read-only preflight: the exact outcome `clear_pool` would execute against
/// current ledger state ("would it fire if it closed now"). Same builder, same
/// clearing function — in the clearing ledger the two are bit-identical, which
/// is the no-discretion guarantee a third party can verify. Past the capture
/// window it reports the abort outcome (no fire), because that is the only
/// outcome `clear_pool` can still execute.
pub fn simulate_clear(env: &Env, pool_id: BytesN<32>) -> Result<ClearOutcome, Error> {
    let pool = storage::get_pool(env, pool_id.clone())?;
    if pool.status != PoolStatus::Open {
        return Err(Error::PoolNotOpen);
    }
    if env.ledger().timestamp() > pool.clearing_deadline + CAPTURE_WINDOW_SECS {
        return Ok(clearing::no_fire(env));
    }
    let members = storage::get_pool_members(env, &pool_id);
    let views = build_child_views(env, &pool, &members);
    Ok(clearing::clear(env, &pool, &views))
}

pub fn get_pool(env: &Env, pool_id: BytesN<32>) -> Result<ClearingPool, Error> {
    storage::get_pool(env, pool_id)
}

pub fn get_pool_members(env: &Env, pool_id: BytesN<32>) -> Result<Vec<BytesN<32>>, Error> {
    if !storage::has_pool(env, &pool_id) {
        return Err(Error::PoolNotFound);
    }
    Ok(storage::get_pool_members(env, &pool_id))
}

/// The eligibility filter — decided once, before any price exists (worst_case,
/// not the p*-dependent leg, so there is no fixed-point circularity). Every
/// term is objective same-ledger state; live allowance/balance reads make
/// ability-to-pay part of the filter, so a member who pulled funds is
/// deterministically excluded instead of holding a veto over the capture.
fn is_eligible(env: &Env, pool: &ClearingPool, mandate: &Mandate) -> bool {
    if mandate.pool_state != PoolState::Committed || mandate.status != Status::Active {
        return false;
    }
    if env.ledger().timestamp() >= mandate.expiry {
        return false;
    }
    let wc = worst_case(&mandate.price_schedule);
    if mandate.max_amount - mandate.spent < wc {
        return false;
    }
    let token = TokenClient::new(env, &pool.asset);
    let contract = env.current_contract_address();
    if token.allowance(&mandate.user, &contract) < wc || token.balance(&mandate.user) < wc {
        return false;
    }
    trustline_authorized(env, &pool.asset, &mandate.user)
}

/// A SAC trustline the issuer deauthorized (froze) still reads a full balance
/// and allowance, but its `transfer_from` reverts — so the funds check alone
/// would score a frozen member eligible and that one leg would revert the
/// whole capture, wedging a met pool for its entire window. Probe the SAC's
/// `authorized(id)` tolerantly: tokens without the method (plain SEP-41)
/// count as authorized.
fn trustline_authorized(env: &Env, asset: &Address, user: &Address) -> bool {
    match env.try_invoke_contract::<bool, soroban_sdk::Error>(
        asset,
        &Symbol::new(env, "authorized"),
        soroban_sdk::vec![env, user.into_val(env)],
    ) {
        Ok(Ok(authorized)) => authorized,
        _ => true,
    }
}

/// The ONE builder both clear_pool and simulate_clear use. Token calls here
/// are reads, so running them before the capture's state writes keeps CEI.
fn build_child_views(env: &Env, pool: &ClearingPool, members: &Vec<BytesN<32>>) -> Vec<ChildView> {
    let mut views: Vec<ChildView> = Vec::new(env);
    for member_id in members.iter() {
        let Ok(mandate) = storage::get_mandate(env, member_id.clone()) else {
            continue;
        };
        views.push_back(ChildView {
            mandate_id: member_id.clone(),
            schedule: mandate.price_schedule.clone(),
            eligible: is_eligible(env, pool, &mandate),
            worst_case: worst_case(&mandate.price_schedule),
        });
    }
    views
}

fn allocation_qty(outcome: &ClearOutcome, mandate_id: &BytesN<32>) -> u128 {
    for allocation in outcome.allocations.iter() {
        if allocation.mandate_id == *mandate_id {
            return allocation.qty;
        }
    }
    0
}

/// sha256( pool_id || p*_i128_BE16 || concat(mandate_id_32 || qty_u128_BE16) )
/// over allocations in mandate_id order. The pool_id prefix stops cross-pool
/// root comparison. Emitted in `pool_clr` so the chosen allocation is publicly
/// checkable against a recomputation of `simulate_clear`.
fn allocation_root(env: &Env, pool_id: &BytesN<32>, outcome: &ClearOutcome) -> BytesN<32> {
    let mut bytes = Bytes::new(env);
    bytes.append(&Bytes::from_array(env, &pool_id.to_array()));
    bytes.append(&Bytes::from_array(
        env,
        &outcome.clearing_price.to_be_bytes(),
    ));
    for allocation in outcome.allocations.iter() {
        bytes.append(&Bytes::from_array(env, &allocation.mandate_id.to_array()));
        bytes.append(&Bytes::from_array(env, &allocation.qty.to_be_bytes()));
    }
    env.crypto().sha256(&bytes).into()
}
