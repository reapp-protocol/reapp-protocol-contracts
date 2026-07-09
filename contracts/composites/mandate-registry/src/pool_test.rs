//! Composite-mandate adversarial suite (design §9.3 / architecture §11).
//! Same conventions as test.rs: every negative asserts the exact typed error
//! (or host revert for auth); money assertions go through the SEP-41 token.

#![cfg(test)]

use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, Vec};

use crate::mandate::{demand, validate_schedule, worst_case};
use crate::pooltypes::{CAPTURE_WINDOW_SECS, MAX_POOL_HORIZON_SECS, MAX_POOL_MEMBERS};
use crate::{
    ClearingKind, Error, MandateRegistry, MandateRegistryClient, PoolState, PoolStatus,
    SchedulePoint, Status,
};

const NOW: u64 = 1_000;
const DEADLINE: u64 = 2_000;
const CHILD_EXPIRY: u64 = 200_000; // > DEADLINE + CAPTURE_WINDOW_SECS
const FUNDED: i128 = 1_000_000_000;
const MAX: i128 = 500_000_000;

struct W {
    env: Env,
    contract: Address,
    originator: Address,
    merchant: Address,
    asset: Address,
}

fn setup() -> W {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    let contract = env.register(MandateRegistry, ());
    let originator = Address::generate(&env);
    let merchant = Address::generate(&env);
    let admin = Address::generate(&env);
    let asset = env.register_stellar_asset_contract_v2(admin).address();
    W {
        env,
        contract,
        originator,
        merchant,
        asset,
    }
}

impl W {
    fn client(&self) -> MandateRegistryClient<'_> {
        MandateRegistryClient::new(&self.env, &self.contract)
    }
    fn token(&self) -> TokenClient<'_> {
        TokenClient::new(&self.env, &self.asset)
    }
    fn balance(&self, who: &Address) -> i128 {
        self.token().balance(who)
    }
    fn new_user(&self) -> Address {
        let user = Address::generate(&self.env);
        StellarAssetClient::new(&self.env, &self.asset).mint(&user, &FUNDED);
        self.token()
            .approve(&user, &self.contract, &FUNDED, &100_000);
        user
    }
    fn nonce(&self, byte: u8) -> BytesN<32> {
        BytesN::from_array(&self.env, &[byte; 32])
    }
    /// ThresholdFloor pool with min_child_value 0 and the standard deadline.
    fn pool(&self, threshold_qty: u128, threshold_value: u128, nonce: u8) -> BytesN<32> {
        self.client().register_pool(
            &self.originator,
            &self.merchant,
            &self.asset,
            &ClearingKind::ThresholdFloor,
            &threshold_qty,
            &threshold_value,
            &0u128,
            &DEADLINE,
            &self.nonce(nonce),
        )
    }
    fn sched(&self, points: &[(i128, u128)]) -> Vec<SchedulePoint> {
        let mut v = Vec::new(&self.env);
        for (unit_price, max_qty) in points {
            v.push_back(SchedulePoint {
                unit_price: *unit_price,
                max_qty: *max_qty,
            });
        }
        v
    }
    /// Register a pooled child with the standard expiry and budget.
    fn child(
        &self,
        user: &Address,
        id_byte: u8,
        pool_id: &BytesN<32>,
        points: &[(i128, u128)],
    ) -> BytesN<32> {
        let agent = Address::generate(&self.env);
        let id = BytesN::from_array(&self.env, &[id_byte; 32]);
        self.client().register_mandate(
            user,
            &agent,
            &self.merchant,
            &self.asset,
            &MAX,
            &CHILD_EXPIRY,
            &id,
            &Some(pool_id.clone()),
            &self.sched(points),
        );
        id
    }
}

// ── pure helpers (schedule semantics) ───────────────────────────────────────

#[test]
fn demand_curve_semantics() {
    let w = setup();
    let s = w.sched(&[(5, 3), (10, 1)]);
    assert_eq!(demand(&s, 1), 3);
    assert_eq!(demand(&s, 5), 3);
    assert_eq!(demand(&s, 7), 1);
    assert_eq!(demand(&s, 10), 1);
    assert_eq!(demand(&s, 11), 0);
    assert_eq!(worst_case(&s), 15); // max(5*3, 10*1)
}

#[test]
fn schedule_validation_rejects_malformed() {
    let w = setup();
    // empty
    assert_eq!(
        validate_schedule(&w.sched(&[])),
        Err(Error::ScheduleInvalid)
    );
    // non-ascending price
    assert_eq!(
        validate_schedule(&w.sched(&[(10, 3), (5, 1)])),
        Err(Error::ScheduleInvalid)
    );
    // non-descending qty (equal is dominated → rejected)
    assert_eq!(
        validate_schedule(&w.sched(&[(5, 3), (10, 3)])),
        Err(Error::ScheduleInvalid)
    );
    // zero price / zero qty
    assert_eq!(
        validate_schedule(&w.sched(&[(0, 3)])),
        Err(Error::ScheduleInvalid)
    );
    assert_eq!(
        validate_schedule(&w.sched(&[(5, 0)])),
        Err(Error::ScheduleInvalid)
    );
    // too many points
    let long: std::vec::Vec<(i128, u128)> = (1..=9).map(|i| (i as i128, 10 - i as u128)).collect();
    assert_eq!(
        validate_schedule(&w.sched(&long)),
        Err(Error::ScheduleInvalid)
    );
    // well-formed
    assert_eq!(validate_schedule(&w.sched(&[(5, 3), (10, 1)])), Ok(()));
}

// ── register_pool ────────────────────────────────────────────────────────────

#[test]
fn register_pool_happy() {
    let w = setup();
    let pid = w.pool(9, 0, 1);
    let p = w.client().get_pool(&pid);
    assert_eq!(p.status, PoolStatus::Open);
    assert_eq!(p.member_count, 0);
    assert_eq!(p.threshold_qty, 9);
    assert_eq!(p.fee_bps_pinned, 0);
    assert_eq!(p.clearing_deadline, DEADLINE);
    assert_eq!(w.client().get_pool_members(&pid).len(), 0);
}

#[test]
fn register_pool_duplicate_and_id_commits_to_terms() {
    let w = setup();
    let pid1 = w.pool(9, 0, 1);
    // identical terms + identical nonce → AlreadyExists
    assert_eq!(
        w.client().try_register_pool(
            &w.originator,
            &w.merchant,
            &w.asset,
            &ClearingKind::ThresholdFloor,
            &9u128,
            &0u128,
            &0u128,
            &DEADLINE,
            &w.nonce(1),
        ),
        Err(Ok(Error::AlreadyExists))
    );
    // same nonce, different terms → different id (the id commits to terms)
    let pid2 = w.pool(10, 0, 1);
    assert_ne!(pid1, pid2);
    // identical terms, different nonce → different id
    let pid3 = w.pool(9, 0, 2);
    assert_ne!(pid1, pid3);
}

#[test]
fn register_pool_bad_inputs_rejected() {
    let w = setup();
    // deadline in the past
    w.env.ledger().set_timestamp(DEADLINE);
    assert_eq!(
        w.client().try_register_pool(
            &w.originator,
            &w.merchant,
            &w.asset,
            &ClearingKind::ThresholdFloor,
            &9u128,
            &0u128,
            &0u128,
            &DEADLINE,
            &w.nonce(1),
        ),
        Err(Ok(Error::DeadlinePassed))
    );
    w.env.ledger().set_timestamp(NOW);
    // deadline beyond the TTL horizon
    assert_eq!(
        w.client().try_register_pool(
            &w.originator,
            &w.merchant,
            &w.asset,
            &ClearingKind::ThresholdFloor,
            &9u128,
            &0u128,
            &0u128,
            &(NOW + MAX_POOL_HORIZON_SECS),
            &w.nonce(1),
        ),
        Err(Ok(Error::DeadlineTooFar))
    );
    // ceiling kinds reserved for Stage 2
    assert_eq!(
        w.client().try_register_pool(
            &w.originator,
            &w.merchant,
            &w.asset,
            &ClearingKind::SpendCeiling,
            &9u128,
            &0u128,
            &0u128,
            &DEADLINE,
            &w.nonce(1),
        ),
        Err(Ok(Error::KindNotSupported))
    );
    // both thresholds zero
    assert_eq!(
        w.client().try_register_pool(
            &w.originator,
            &w.merchant,
            &w.asset,
            &ClearingKind::ThresholdFloor,
            &0u128,
            &0u128,
            &0u128,
            &DEADLINE,
            &w.nonce(1),
        ),
        Err(Ok(Error::InvalidAmount))
    );
}

// ── pooled child registration ────────────────────────────────────────────────

#[test]
fn pooled_registration_negatives() {
    let w = setup();
    let pid = w.pool(9, 0, 1);
    let user = w.new_user();
    let agent = Address::generate(&w.env);
    let c = w.client();
    let reg = |id_byte: u8,
               merchant: &Address,
               asset: &Address,
               expiry: u64,
               max: i128,
               pool: &Option<BytesN<32>>,
               points: &[(i128, u128)]| {
        c.try_register_mandate(
            &user,
            &agent,
            merchant,
            asset,
            &max,
            &expiry,
            &BytesN::from_array(&w.env, &[id_byte; 32]),
            pool,
            &w.sched(points),
        )
    };
    let other = Address::generate(&w.env);
    let some = Some(pid.clone());

    // unknown pool
    let ghost = Some(BytesN::from_array(&w.env, &[9u8; 32]));
    assert_eq!(
        reg(
            10,
            &w.merchant,
            &w.asset,
            CHILD_EXPIRY,
            MAX,
            &ghost,
            &[(5, 3)]
        ),
        Err(Ok(Error::PoolNotFound))
    );
    // merchant / asset must match the pool
    assert_eq!(
        reg(11, &other, &w.asset, CHILD_EXPIRY, MAX, &some, &[(5, 3)]),
        Err(Ok(Error::PoolMerchantMismatch))
    );
    assert_eq!(
        reg(12, &w.merchant, &other, CHILD_EXPIRY, MAX, &some, &[(5, 3)]),
        Err(Ok(Error::PoolAssetMismatch))
    );
    // schedule holes
    assert_eq!(
        reg(13, &w.merchant, &w.asset, CHILD_EXPIRY, MAX, &some, &[]),
        Err(Ok(Error::ScheduleInvalid))
    );
    assert_eq!(
        reg(
            14,
            &w.merchant,
            &w.asset,
            CHILD_EXPIRY,
            MAX,
            &some,
            &[(10, 3), (5, 1)]
        ),
        Err(Ok(Error::ScheduleInvalid))
    );
    // schedule on a standalone mandate
    assert_eq!(
        reg(
            15,
            &w.merchant,
            &w.asset,
            CHILD_EXPIRY,
            MAX,
            &None,
            &[(5, 3)]
        ),
        Err(Ok(Error::ScheduleInvalid))
    );
    // worst_case above the signed budget
    assert_eq!(
        reg(
            16,
            &w.merchant,
            &w.asset,
            CHILD_EXPIRY,
            14,
            &some,
            &[(5, 3)]
        ),
        Err(Ok(Error::ScheduleInvalid))
    );
    // expiry inside the capture window
    assert_eq!(
        reg(
            17,
            &w.merchant,
            &w.asset,
            DEADLINE + CAPTURE_WINDOW_SECS,
            MAX,
            &some,
            &[(5, 3)]
        ),
        Err(Ok(Error::ExpiryBeforeDeadline))
    );
}

#[test]
fn below_min_child_rejected() {
    let w = setup();
    let pid = w.client().register_pool(
        &w.originator,
        &w.merchant,
        &w.asset,
        &ClearingKind::ThresholdFloor,
        &9u128,
        &0u128,
        &100u128, // min_child_value: worst_case must reach 100
        &DEADLINE,
        &w.nonce(3),
    );
    let user = w.new_user();
    let agent = Address::generate(&w.env);
    assert_eq!(
        w.client().try_register_mandate(
            &user,
            &agent,
            &w.merchant,
            &w.asset,
            &MAX,
            &CHILD_EXPIRY,
            &BytesN::from_array(&w.env, &[20u8; 32]),
            &Some(pid),
            &w.sched(&[(5, 3)]), // worst_case 15 < 100
        ),
        Err(Ok(Error::BelowMinChild))
    );
}

// ── commit_child ─────────────────────────────────────────────────────────────

#[test]
fn commit_happy_and_state() {
    let w = setup();
    let pid = w.pool(9, 0, 1);
    let user = w.new_user();
    let id = w.child(&user, 21, &pid, &[(5, 3)]);
    w.client().commit_child(&id);
    let m = w.client().get_mandate(&id);
    assert_eq!(m.pool_state, PoolState::Committed);
    assert_eq!(w.client().get_pool(&pid).member_count, 1);
    assert_eq!(w.client().get_pool_members(&pid).len(), 1);
}

#[test]
fn commit_negatives() {
    let w = setup();
    let pid = w.pool(9, 0, 1);
    let user = w.new_user();
    let c = w.client();

    // standalone mandate → NotPooled
    let agent = Address::generate(&w.env);
    let solo = BytesN::from_array(&w.env, &[30u8; 32]);
    c.register_mandate(
        &user,
        &agent,
        &w.merchant,
        &w.asset,
        &MAX,
        &CHILD_EXPIRY,
        &solo,
        &None,
        &Vec::new(&w.env),
    );
    assert_eq!(c.try_commit_child(&solo), Err(Ok(Error::NotPooled)));

    let id = w.child(&user, 31, &pid, &[(5, 3)]);
    c.commit_child(&id);
    // double commit
    assert_eq!(c.try_commit_child(&id), Err(Ok(Error::BadPoolState)));
    // same user, second mandate, same pool → self-sybil dedup
    let id2 = w.child(&user, 32, &pid, &[(5, 3)]);
    assert_eq!(c.try_commit_child(&id2), Err(Ok(Error::DuplicateMember)));

    // past the deadline
    let late_user = w.new_user();
    let id3 = w.child(&late_user, 33, &pid, &[(5, 3)]);
    w.env.ledger().set_timestamp(DEADLINE);
    assert_eq!(c.try_commit_child(&id3), Err(Ok(Error::DeadlinePassed)));
}

#[test]
fn commit_insufficient_funds_preflight() {
    let w = setup();
    let pid = w.pool(9, 0, 1);
    // no allowance granted at all
    let user = Address::generate(&w.env);
    StellarAssetClient::new(&w.env, &w.asset).mint(&user, &FUNDED);
    let id = w.child(&user, 34, &pid, &[(5, 3)]);
    assert_eq!(
        w.client().try_commit_child(&id),
        Err(Ok(Error::InsufficientFunds))
    );
}

#[test]
fn commit_after_solo_spend_rejected_when_budget_gone() {
    // R2: a pooled child may spend solo while Unlinked; if that leaves less
    // than worst_case of budget, commit must refuse — otherwise the child
    // would brick every capture retry.
    let w = setup();
    let pid = w.pool(3, 0, 1);
    let user = w.new_user();
    let agent = Address::generate(&w.env);
    let id = BytesN::from_array(&w.env, &[35u8; 32]);
    // budget exactly worst_case: any solo spend leaves too little
    w.client().register_mandate(
        &user,
        &agent,
        &w.merchant,
        &w.asset,
        &15i128,
        &CHILD_EXPIRY,
        &id,
        &Some(pid.clone()),
        &w.sched(&[(5, 3)]),
    );
    // solo path is open while Unlinked
    w.client().execute_payment(&id, &1i128, &0u32);
    assert_eq!(
        w.client().try_commit_child(&id),
        Err(Ok(Error::InsufficientFunds))
    );
}

#[test]
fn pool_full_at_capacity() {
    let w = setup();
    let pid = w.pool(1_000_000, 0, 1); // unreachable threshold; only capacity matters
    for i in 0..MAX_POOL_MEMBERS {
        let user = w.new_user();
        let id = w.child(&user, 40 + i as u8, &pid, &[(5, 3)]);
        w.client().commit_child(&id);
    }
    let user9 = w.new_user();
    let id9 = w.child(&user9, 39, &pid, &[(5, 3)]);
    assert_eq!(w.client().try_commit_child(&id9), Err(Ok(Error::PoolFull)));
}

// ── evict_child ──────────────────────────────────────────────────────────────

#[test]
fn evict_only_ineligible_members() {
    let w = setup();
    let pid = w.pool(9, 0, 1);
    let user = w.new_user();
    let id = w.child(&user, 50, &pid, &[(5, 3)]);
    w.client().commit_child(&id);

    // still eligible → cannot evict
    assert_eq!(
        w.client().try_evict_child(&pid, &id),
        Err(Ok(Error::MemberStillEligible))
    );

    // user pulls the allowance → objectively ineligible → evictable by anyone
    w.token().approve(&user, &w.contract, &0i128, &100_000);
    w.client().evict_child(&pid, &id);
    assert_eq!(w.client().get_mandate(&id).pool_state, PoolState::Released);
    assert_eq!(w.client().get_pool(&pid).member_count, 0);
    assert_eq!(w.client().get_pool_members(&pid).len(), 0);
}

// ── solo/pool exclusion ──────────────────────────────────────────────────────

#[test]
fn committed_blocks_solo_released_reopens() {
    let w = setup();
    let pid = w.pool(9, 0, 1);
    let user = w.new_user();
    let id = w.child(&user, 51, &pid, &[(5, 3)]);

    // Unlinked pooled child may spend solo within its own limits
    w.client().execute_payment(&id, &1_000i128, &0u32);

    w.client().commit_child(&id);
    // Committed blocks the solo path — both preflight and spend
    assert_eq!(
        w.client()
            .try_validate_mandate(&id, &1_000i128, &w.merchant),
        Err(Ok(Error::MandatePooled))
    );
    assert_eq!(
        w.client().try_execute_payment(&id, &1_000i128, &1u32),
        Err(Ok(Error::MandatePooled))
    );

    // pool aborts (nobody else joined; threshold unmet) → child Released
    w.env.ledger().set_timestamp(DEADLINE);
    w.client().clear_pool(&pid);
    assert_eq!(w.client().get_pool(&pid).status, PoolStatus::Aborted);
    assert_eq!(w.client().get_mandate(&id).pool_state, PoolState::Released);
    // Released re-opens the solo path, own limits still enforced
    w.client().execute_payment(&id, &1_000i128, &1u32);
    assert_eq!(w.balance(&w.merchant), 2_000);
}

#[test]
fn revoke_committed_frees_slot() {
    let w = setup();
    let pid = w.pool(9, 0, 1);
    let user = w.new_user();
    let id = w.child(&user, 52, &pid, &[(5, 3)]);
    w.client().commit_child(&id);
    w.client().revoke_mandate(&id);
    let m = w.client().get_mandate(&id);
    assert_eq!(m.status, Status::Revoked);
    assert_eq!(m.pool_state, PoolState::Released);
    assert_eq!(w.client().get_pool(&pid).member_count, 0);
    assert_eq!(w.client().get_pool_members(&pid).len(), 0);
}

// ── clear_pool: timing (the deadline auction) ────────────────────────────────

#[test]
fn clear_before_deadline_rejected_even_when_feasible() {
    let w = setup();
    let pid = w.pool(3, 0, 1);
    let user = w.new_user();
    let id = w.child(&user, 53, &pid, &[(5, 3)]);
    w.client().commit_child(&id);
    // threshold already met — but the auction has not closed
    assert_eq!(
        w.client().try_clear_pool(&pid),
        Err(Ok(Error::DeadlineNotReached))
    );
    assert_eq!(w.balance(&w.merchant), 0);
}

#[test]
fn clear_past_capture_window_aborts_even_when_met() {
    let w = setup();
    let pid = w.pool(3, 0, 1);
    let user = w.new_user();
    let id = w.child(&user, 54, &pid, &[(5, 3)]);
    w.client().commit_child(&id);
    w.env
        .ledger()
        .set_timestamp(DEADLINE + CAPTURE_WINDOW_SECS + 1);
    w.client().clear_pool(&pid);
    assert_eq!(w.client().get_pool(&pid).status, PoolStatus::Aborted);
    assert_eq!(w.client().get_mandate(&id).pool_state, PoolState::Released);
    assert_eq!(w.balance(&w.merchant), 0);
}

// ── clear_pool: clearing math ────────────────────────────────────────────────

/// The flagship group buy: three buyers, each "3 units at 50M, or 1 at 100M".
/// Vendor minimum: 9 units and 405M value → uniform p* = 45M (sub-breakpoint,
/// buyer-optimal), everyone gets 3 units for 135M.
#[test]
fn group_buy_fires_at_uniform_minimal_price() {
    let w = setup();
    let pid = w.pool(9, 405_000_000, 1);
    let mut users = std::vec::Vec::new();
    let mut ids = std::vec::Vec::new();
    for i in 0..3u8 {
        let user = w.new_user();
        let id = w.child(&user, 60 + i, &pid, &[(50_000_000, 3), (100_000_000, 1)]);
        w.client().commit_child(&id);
        users.push(user);
        ids.push(id);
    }
    w.env.ledger().set_timestamp(DEADLINE);
    w.client().clear_pool(&pid);

    assert_eq!(w.client().get_pool(&pid).status, PoolStatus::Cleared);
    assert_eq!(w.balance(&w.merchant), 405_000_000);
    for (user, id) in users.iter().zip(ids.iter()) {
        // uniform price 45M × 3 units = 135M per member, all identical
        assert_eq!(w.balance(user), FUNDED - 135_000_000);
        let m = w.client().get_mandate(id);
        assert_eq!(m.pool_state, PoolState::Captured);
        assert_eq!(m.spent, 135_000_000);
        assert_eq!(m.seq, 1);
    }
}

/// Architecture canonical case: single child [(10,10)], thresholds qty 10 /
/// value 60 → p* = 6, not the posted 10. The buyer is never overcharged to a
/// posted breakpoint when a lower uniform price already satisfies the vendor.
#[test]
fn minimal_price_beats_posted_breakpoint() {
    let w = setup();
    let pid = w.pool(10, 60, 1);
    let user = w.new_user();
    let id = w.child(&user, 63, &pid, &[(10, 10)]);
    w.client().commit_child(&id);
    w.env.ledger().set_timestamp(DEADLINE);

    let outcome = w.client().simulate_clear(&pid);
    assert!(outcome.fires);
    assert_eq!(outcome.clearing_price, 6);
    assert_eq!(outcome.total_qty, 10);
    assert_eq!(outcome.net_value, 60);

    w.client().clear_pool(&pid);
    assert_eq!(w.balance(&w.merchant), 60);
    assert_eq!(w.balance(&user), FUNDED - 60);
}

/// Value threshold binding below every breakpoint: [(5,3),(10,1)], qty 1 /
/// value 8 → p* = 3 (3 units × 3 = 9 ≥ 8), a price no schedule posted.
#[test]
fn value_binding_clears_below_breakpoints() {
    let w = setup();
    let pid = w.pool(1, 8, 1);
    let user = w.new_user();
    let id = w.child(&user, 64, &pid, &[(5, 3), (10, 1)]);
    w.client().commit_child(&id);
    w.env.ledger().set_timestamp(DEADLINE);
    let outcome = w.client().simulate_clear(&pid);
    assert_eq!(outcome.clearing_price, 3);
    w.client().clear_pool(&pid);
    assert_eq!(w.balance(&w.merchant), 9);
}

/// Uniform price across different top tiers: the child who posted a higher
/// top price still pays the single p*.
#[test]
fn uniform_price_across_tiers() {
    let w = setup();
    let pid = w.pool(12, 60, 1);
    let a = w.new_user();
    let b = w.new_user();
    let id_a = w.child(&a, 65, &pid, &[(5, 10)]);
    let id_b = w.child(&b, 66, &pid, &[(8, 2)]);
    w.client().commit_child(&id_a);
    w.client().commit_child(&id_b);
    w.env.ledger().set_timestamp(DEADLINE);
    let outcome = w.client().simulate_clear(&pid);
    assert!(outcome.fires);
    assert_eq!(outcome.clearing_price, 5);
    w.client().clear_pool(&pid);
    assert_eq!(w.balance(&a), FUNDED - 50); // 10 × 5
    assert_eq!(w.balance(&b), FUNDED - 10); // 2 × 5, not 2 × 8
    assert_eq!(w.balance(&w.merchant), 60);
}

/// A committed child priced out at p* (demand 0) is Released, never charged.
#[test]
fn priced_out_child_released_not_charged() {
    let w = setup();
    let pid = w.pool(3, 12, 1);
    let a = w.new_user();
    let b = w.new_user();
    let id_a = w.child(&a, 67, &pid, &[(5, 3)]);
    let id_b = w.child(&b, 68, &pid, &[(2, 1)]); // only buys at p <= 2
    w.client().commit_child(&id_a);
    w.client().commit_child(&id_b);
    w.env.ledger().set_timestamp(DEADLINE);
    // interval (0,2]: qty 4, net(2) = 8 < 12. interval (2,5]: qty 3, net(5) = 15 ≥ 12 → p* = 4.
    let outcome = w.client().simulate_clear(&pid);
    assert_eq!(outcome.clearing_price, 4);
    assert_eq!(outcome.allocations.len(), 1);
    w.client().clear_pool(&pid);
    assert_eq!(w.balance(&a), FUNDED - 12);
    assert_eq!(w.balance(&b), FUNDED); // untouched
    assert_eq!(
        w.client().get_mandate(&id_b).pool_state,
        PoolState::Released
    );
    assert_eq!(
        w.client().get_mandate(&id_a).pool_state,
        PoolState::Captured
    );
}

#[test]
fn under_threshold_aborts_nobody_pays() {
    let w = setup();
    let pid = w.pool(9, 0, 1);
    let a = w.new_user();
    let id_a = w.child(&a, 69, &pid, &[(5, 3)]); // 3 < 9
    w.client().commit_child(&id_a);
    w.env.ledger().set_timestamp(DEADLINE);
    w.client().clear_pool(&pid);
    assert_eq!(w.client().get_pool(&pid).status, PoolStatus::Aborted);
    assert_eq!(w.balance(&a), FUNDED);
    assert_eq!(w.balance(&w.merchant), 0);
    assert_eq!(
        w.client().get_mandate(&id_a).pool_state,
        PoolState::Released
    );
}

// ── ability to pay (D-B): exclusion instead of veto ──────────────────────────

#[test]
fn pulled_allowance_member_excluded_pool_still_fires() {
    let w = setup();
    let pid = w.pool(3, 0, 1);
    let a = w.new_user();
    let b = w.new_user();
    let id_a = w.child(&a, 70, &pid, &[(5, 3)]);
    let id_b = w.child(&b, 71, &pid, &[(5, 3)]);
    w.client().commit_child(&id_a);
    w.client().commit_child(&id_b);
    // b defects: pulls the allowance before the close
    w.token().approve(&b, &w.contract, &0i128, &100_000);
    w.env.ledger().set_timestamp(DEADLINE);
    w.client().clear_pool(&pid);
    // fires on a alone (3 ≥ 3); b excluded deterministically, never charged
    assert_eq!(w.client().get_pool(&pid).status, PoolStatus::Cleared);
    assert_eq!(w.balance(&b), FUNDED);
    assert_eq!(
        w.client().get_mandate(&id_b).pool_state,
        PoolState::Released
    );
    assert_eq!(
        w.client().get_mandate(&id_a).pool_state,
        PoolState::Captured
    );
}

#[test]
fn pulled_allowance_below_threshold_aborts() {
    let w = setup();
    let pid = w.pool(6, 0, 1);
    let a = w.new_user();
    let b = w.new_user();
    let id_a = w.child(&a, 72, &pid, &[(5, 3)]);
    let id_b = w.child(&b, 73, &pid, &[(5, 3)]);
    w.client().commit_child(&id_a);
    w.client().commit_child(&id_b);
    w.token().approve(&b, &w.contract, &0i128, &100_000);
    w.env.ledger().set_timestamp(DEADLINE);
    w.client().clear_pool(&pid);
    assert_eq!(w.client().get_pool(&pid).status, PoolStatus::Aborted);
    assert_eq!(w.balance(&a), FUNDED);
    assert_eq!(w.balance(&w.merchant), 0);
}

#[test]
fn drained_balance_member_excluded() {
    let w = setup();
    let pid = w.pool(3, 0, 1);
    let a = w.new_user();
    let b = w.new_user();
    let id_a = w.child(&a, 74, &pid, &[(5, 3)]);
    let id_b = w.child(&b, 75, &pid, &[(5, 3)]);
    w.client().commit_child(&id_a);
    w.client().commit_child(&id_b);
    // b moves the balance away (allowance intact)
    let sink = Address::generate(&w.env);
    w.token().transfer(&b, &sink, &FUNDED);
    w.env.ledger().set_timestamp(DEADLINE);
    w.client().clear_pool(&pid);
    assert_eq!(w.client().get_pool(&pid).status, PoolStatus::Cleared);
    assert_eq!(
        w.client().get_mandate(&id_b).pool_state,
        PoolState::Released
    );
}

// ── idempotency / no-discretion / order independence ─────────────────────────

#[test]
fn double_clear_rejected() {
    let w = setup();
    let pid = w.pool(3, 15, 1);
    let a = w.new_user();
    let id = w.child(&a, 76, &pid, &[(5, 3)]);
    w.client().commit_child(&id);
    w.env.ledger().set_timestamp(DEADLINE);
    w.client().clear_pool(&pid);
    assert_eq!(w.client().try_clear_pool(&pid), Err(Ok(Error::PoolNotOpen)));
    // and simulate on a terminal pool is refused too
    assert_eq!(
        w.client().try_simulate_clear(&pid),
        Err(Ok(Error::PoolNotOpen))
    );
    assert_eq!(w.balance(&a), FUNDED - 15); // charged exactly once
}

/// The no-discretion equality: simulate_clear in the clearing ledger IS the
/// allocation clear_pool executes — price, allocations, and per-leg amounts.
#[test]
fn simulate_equals_capture_same_ledger() {
    let w = setup();
    let pid = w.pool(9, 405_000_000, 1);
    let mut users = std::vec::Vec::new();
    for i in 0..3u8 {
        let user = w.new_user();
        let id = w.child(&user, 80 + i, &pid, &[(50_000_000, 3), (100_000_000, 1)]);
        w.client().commit_child(&id);
        users.push((user, id));
    }
    w.env.ledger().set_timestamp(DEADLINE);

    let outcome = w.client().simulate_clear(&pid);
    assert!(outcome.fires);
    w.client().clear_pool(&pid);

    // every transfer matches the simulated allocation exactly
    let mut simulated_total: i128 = 0;
    for allocation in outcome.allocations.iter() {
        let leg = outcome.clearing_price * allocation.qty as i128;
        simulated_total += leg;
        let m = w.client().get_mandate(&allocation.mandate_id);
        assert_eq!(m.spent, leg);
        assert_eq!(m.pool_state, PoolState::Captured);
    }
    assert_eq!(w.balance(&w.merchant), simulated_total);
    assert_eq!(simulated_total, outcome.gross_value);
}

/// Commit order does not matter: same members, different commit order, same
/// outcome (the only order inside clearing is the mandate_id sort).
#[test]
fn order_independence() {
    let run = |order: [u8; 3]| {
        let w = setup();
        let pid = w.pool(9, 405_000_000, 1);
        // fixed users per id byte so both runs are identical up to commit order
        let mut ids = std::vec::Vec::new();
        for id_byte in [90u8, 91, 92] {
            let user = w.new_user();
            ids.push(w.child(&user, id_byte, &pid, &[(50_000_000, 3), (100_000_000, 1)]));
        }
        for pos in order {
            w.client().commit_child(&ids[pos as usize]);
        }
        w.env.ledger().set_timestamp(DEADLINE);
        let o = w.client().simulate_clear(&pid);
        let mut allocs = std::vec::Vec::new();
        for a in o.allocations.iter() {
            allocs.push((a.mandate_id.to_array(), a.qty));
        }
        (o.clearing_price, o.total_qty, o.net_value, allocs)
    };
    assert_eq!(run([0, 1, 2]), run([2, 0, 1]));
}

// ── capture atomicity: reentry probe ─────────────────────────────────────────

// A malicious "token" that reenters clear_pool during the capture transfer.
// CEI (state persisted before transfers) + the PoolNotOpen guard must make the
// inner call fail; the outer capture settles exactly once.
#[contract]
pub struct EvilPoolToken;

#[contractimpl]
impl EvilPoolToken {
    pub fn set(env: Env, registry: Address, pool_id: BytesN<32>) {
        env.storage().instance().set(&0u32, &registry);
        env.storage().instance().set(&1u32, &pool_id);
    }
    pub fn allowance(_env: Env, _from: Address, _spender: Address) -> i128 {
        i128::MAX / 4
    }
    pub fn balance(_env: Env, _id: Address) -> i128 {
        i128::MAX / 4
    }
    pub fn transfer_from(env: Env, _spender: Address, _from: Address, _to: Address, _amount: i128) {
        let registry: Address = env.storage().instance().get(&0u32).unwrap();
        let pool_id: BytesN<32> = env.storage().instance().get(&1u32).unwrap();
        let c = MandateRegistryClient::new(&env, &registry);
        // Must fail — the pool is already Cleared (CEI) and the host forbids
        // reentry. Either way, no second capture.
        assert!(c.try_clear_pool(&pool_id).is_err());
    }
}

#[test]
fn reentrant_clear_cannot_double_capture() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    let registry = env.register(MandateRegistry, ());
    let evil = env.register(EvilPoolToken, ());
    let client = MandateRegistryClient::new(&env, &registry);

    let originator = Address::generate(&env);
    let merchant = Address::generate(&env);
    let user = Address::generate(&env);
    let agent = Address::generate(&env);

    let pid = client.register_pool(
        &originator,
        &merchant,
        &evil,
        &ClearingKind::ThresholdFloor,
        &3u128,
        &15u128,
        &0u128,
        &DEADLINE,
        &BytesN::from_array(&env, &[5u8; 32]),
    );
    let id = BytesN::from_array(&env, &[95u8; 32]);
    client.register_mandate(
        &user,
        &agent,
        &merchant,
        &evil,
        &MAX,
        &CHILD_EXPIRY,
        &id,
        &Some(pid.clone()),
        &{
            let mut v = Vec::new(&env);
            v.push_back(SchedulePoint {
                unit_price: 5,
                max_qty: 3,
            });
            v
        },
    );
    client.commit_child(&id);
    EvilPoolTokenClient::new(&env, &evil).set(&registry, &pid);

    env.ledger().set_timestamp(DEADLINE);
    client.clear_pool(&pid);

    let m = client.get_mandate(&id);
    assert_eq!(
        (m.spent, m.seq, m.pool_state),
        (15i128, 1u32, PoolState::Captured),
        "REENTRY OBSERVED: capture state differs from single-capture baseline"
    );
    assert_eq!(client.get_pool(&pid).status, PoolStatus::Cleared);
}

// ── review findings: regression tests ────────────────────────────────────────

/// A frozen (issuer-deauthorized) SAC trustline reads full balance/allowance
/// but its transfer_from reverts. The eligibility filter must probe
/// `authorized(id)` so a frozen member is deterministically excluded (and
/// evictable) instead of wedging a met pool's capture for the whole window.
#[test]
fn frozen_trustline_member_excluded_and_evictable() {
    use soroban_sdk::testutils::IssuerFlags;

    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(NOW);
    let contract = env.register(MandateRegistry, ());
    let client = MandateRegistryClient::new(&env, &contract);
    let originator = Address::generate(&env);
    let merchant = Address::generate(&env);
    let admin = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(admin);
    sac.issuer().set_flag(IssuerFlags::RevocableFlag);
    let asset = sac.address();
    let sac_client = StellarAssetClient::new(&env, &asset);
    let token = TokenClient::new(&env, &asset);

    let pid = client.register_pool(
        &originator,
        &merchant,
        &asset,
        &ClearingKind::ThresholdFloor,
        &3u128,
        &15u128,
        &0u128,
        &DEADLINE,
        &BytesN::from_array(&env, &[110u8; 32]),
    );
    let mut users = std::vec::Vec::new();
    let mut ids = std::vec::Vec::new();
    for i in 0..2u8 {
        let user = Address::generate(&env);
        sac_client.mint(&user, &FUNDED);
        token.approve(&user, &contract, &FUNDED, &100_000);
        let agent = Address::generate(&env);
        let id = BytesN::from_array(&env, &[111 + i; 32]);
        client.register_mandate(
            &user,
            &agent,
            &merchant,
            &asset,
            &MAX,
            &CHILD_EXPIRY,
            &id,
            &Some(pid.clone()),
            &{
                let mut v = Vec::new(&env);
                v.push_back(SchedulePoint {
                    unit_price: 5,
                    max_qty: 3,
                });
                v
            },
        );
        client.commit_child(&id);
        users.push(user);
        ids.push(id);
    }

    // The issuer freezes buyer 2's trustline after commit.
    sac_client.set_authorized(&users[1], &false);
    // Frozen member is now objectively ineligible → evictable by anyone…
    client.evict_child(&pid, &ids[1]);
    assert_eq!(client.get_mandate(&ids[1]).pool_state, PoolState::Released);

    // …and even if it had stayed, the filter excludes it: the pool still
    // fires on buyer 1 alone, and the frozen account is never charged.
    env.ledger().set_timestamp(DEADLINE);
    let outcome = client.simulate_clear(&pid);
    assert!(outcome.fires);
    assert_eq!(outcome.allocations.len(), 1);
    client.clear_pool(&pid);
    assert_eq!(client.get_pool(&pid).status, PoolStatus::Cleared);
    assert_eq!(client.get_mandate(&ids[0]).pool_state, PoolState::Captured);
    assert_eq!(token.balance(&users[1]), FUNDED);
}

/// Past the capture window, clear_pool can only abort — simulate_clear must
/// report that truth (no fire) instead of an allocation that cannot execute.
#[test]
fn simulate_past_window_reports_no_fire() {
    let w = setup();
    let pid = w.pool(3, 15, 1);
    let user = w.new_user();
    let id = w.child(&user, 120, &pid, &[(5, 3)]);
    w.client().commit_child(&id);
    w.env
        .ledger()
        .set_timestamp(DEADLINE + CAPTURE_WINDOW_SECS + 1);
    let outcome = w.client().simulate_clear(&pid);
    assert!(!outcome.fires);
    w.client().clear_pool(&pid);
    assert_eq!(w.client().get_pool(&pid).status, PoolStatus::Aborted);
}

/// An absurd deadline must surface the typed error, never an overflow panic.
#[test]
fn register_pool_extreme_deadline_typed_error() {
    let w = setup();
    assert_eq!(
        w.client().try_register_pool(
            &w.originator,
            &w.merchant,
            &w.asset,
            &ClearingKind::ThresholdFloor,
            &9u128,
            &0u128,
            &0u128,
            &u64::MAX,
            &w.nonce(1),
        ),
        Err(Ok(Error::DeadlineTooFar))
    );
}

// ── resource ceiling (§9.5): a full pool must actually clear ─────────────────

#[test]
fn full_pool_max_schedules_clears() {
    let w = setup();
    // 8 members × 8-point schedules; at p = 1 every member demands 8 → Q = 64.
    let pid = w.pool(64, 64, 1);
    for i in 0..MAX_POOL_MEMBERS {
        let user = w.new_user();
        let points: std::vec::Vec<(i128, u128)> =
            (1..=8).map(|k| (k as i128, (9 - k) as u128)).collect();
        let id = w.child(&user, 100 + i as u8, &pid, &points);
        w.client().commit_child(&id);
    }
    w.env.ledger().set_timestamp(DEADLINE);
    w.client().clear_pool(&pid);
    assert_eq!(w.client().get_pool(&pid).status, PoolStatus::Cleared);
    assert_eq!(w.balance(&w.merchant), 64); // 64 units × p* = 1
}
