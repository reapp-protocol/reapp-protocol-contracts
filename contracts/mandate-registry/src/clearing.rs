//! The trust core: the pure clearing function. No storage, no clock, no token
//! calls — inputs are plain values, output is the outcome. `clear_pool` and
//! `simulate_clear` both call this over views built by the same builder
//! (`pool::build_child_views`), which is the no-discretion guarantee: given
//! the same ledger state, everyone computes the identical allocation, so the
//! originator has nothing left to choose.
//!
//! Algorithm (ThresholdFloor): scan the ascending union of schedule
//! breakpoints; demand is constant on each interval `(prev, b]`, so the first
//! interval whose constant quantity meets `threshold_qty` and whose net value
//! at `b` meets `threshold_value` contains the answer, and the exact minimal
//! feasible integer price inside it is found by binary search (net value is
//! monotone nondecreasing in price at constant demand: each leg's net rises
//! by 0 or 1 per unit price step, never falls). Scanning intervals in
//! ascending order makes the found price globally minimal: every price in an
//! earlier interval is lower and was proven infeasible.

use soroban_sdk::{Env, Vec};

use crate::mandate::demand;
use crate::pooltypes::{Allocation, ChildView, ClearOutcome, ClearingPool, BPS_DENOM};

pub fn no_fire(env: &Env) -> ClearOutcome {
    ClearOutcome {
        fires: false,
        clearing_price: 0,
        allocations: Vec::new(env),
        total_qty: 0,
        gross_value: 0,
        total_fee: 0,
        net_value: 0,
    }
}

pub fn clear(env: &Env, pool: &ClearingPool, views: &Vec<ChildView>) -> ClearOutcome {
    // Eligible children in mandate_id order — the fixed, content-independent
    // single tie-break. Insertion sort; n <= MAX_POOL_MEMBERS = 8.
    let mut kids: Vec<ChildView> = Vec::new(env);
    for view in views.iter() {
        if !view.eligible {
            continue;
        }
        let mut i = 0u32;
        while i < kids.len() {
            if kids.get_unchecked(i).mandate_id.to_array() > view.mandate_id.to_array() {
                break;
            }
            i += 1;
        }
        kids.insert(i, view);
    }
    if kids.is_empty() {
        return no_fire(env);
    }

    // Ascending, deduplicated union of every eligible schedule breakpoint.
    let mut breakpoints: Vec<i128> = Vec::new(env);
    for kid in kids.iter() {
        for point in kid.schedule.iter() {
            let p = point.unit_price;
            let mut i = 0u32;
            let mut duplicate = false;
            while i < breakpoints.len() {
                let existing = breakpoints.get_unchecked(i);
                if existing == p {
                    duplicate = true;
                    break;
                }
                if existing > p {
                    break;
                }
                i += 1;
            }
            if !duplicate {
                breakpoints.insert(i, p);
            }
        }
    }

    // register_pool caps threshold_value below every reachable pool sum, so
    // this cast cannot truncate.
    let threshold_value = pool.threshold_value as i128;

    let mut prev: i128 = 0;
    for b in breakpoints.iter() {
        // Demand — and therefore quantity — is constant on (prev, b].
        if total_qty(&kids, b) >= pool.threshold_qty
            && net_value(&kids, b, pool.fee_bps_pinned) >= threshold_value
        {
            // Exact minimal feasible price in (prev, b] by binary search.
            let mut lo = prev + 1;
            let mut hi = b;
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if net_value(&kids, mid, pool.fee_bps_pinned) >= threshold_value {
                    hi = mid;
                } else {
                    lo = mid + 1;
                }
            }
            return outcome_at(env, &kids, lo, pool.fee_bps_pinned);
        }
        prev = b;
    }
    no_fire(env)
}

fn total_qty(kids: &Vec<ChildView>, p: i128) -> u128 {
    let mut qty: u128 = 0;
    for kid in kids.iter() {
        qty += demand(&kid.schedule, p);
    }
    qty
}

fn fee_of(leg: i128, fee_bps: u32) -> i128 {
    // Floored, so merchant leg + fee leg always sum exactly to the gross leg.
    leg * fee_bps as i128 / BPS_DENOM
}

fn net_value(kids: &Vec<ChildView>, p: i128, fee_bps: u32) -> i128 {
    let mut net: i128 = 0;
    for kid in kids.iter() {
        let qty = demand(&kid.schedule, p);
        if qty == 0 {
            continue;
        }
        let leg = p * qty as i128;
        net += leg - fee_of(leg, fee_bps);
    }
    net
}

fn outcome_at(env: &Env, kids: &Vec<ChildView>, p: i128, fee_bps: u32) -> ClearOutcome {
    let mut allocations: Vec<Allocation> = Vec::new(env);
    let mut total_qty: u128 = 0;
    let mut gross_value: i128 = 0;
    let mut total_fee: i128 = 0;
    for kid in kids.iter() {
        let qty = demand(&kid.schedule, p);
        if qty == 0 {
            continue;
        }
        let leg = p * qty as i128;
        total_qty += qty;
        gross_value += leg;
        total_fee += fee_of(leg, fee_bps);
        allocations.push_back(Allocation {
            mandate_id: kid.mandate_id.clone(),
            qty,
        });
    }
    ClearOutcome {
        fires: true,
        clearing_price: p,
        allocations,
        total_qty,
        gross_value,
        total_fee,
        net_value: gross_value - total_fee,
    }
}
