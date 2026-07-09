//! Emitted events. Leaf module — anyone may emit. All topics <= 9 chars
//! (`symbol_short!` legal).

use soroban_sdk::{symbol_short, Address, BytesN, Env};

/// `register_mandate` stored a mandate. topic: ("register", user) data: mandate_id
pub fn mandate_registered(env: &Env, mandate_id: &BytesN<32>, user: &Address) {
    env.events().publish(
        (symbol_short!("register"), user.clone()),
        mandate_id.clone(),
    );
}

/// `execute_payment` moved funds. topic: ("payment", merchant) data: (mandate_id, amount)
pub fn payment_executed(env: &Env, mandate_id: &BytesN<32>, merchant: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("payment"), merchant.clone()),
        (mandate_id.clone(), amount),
    );
}

/// `revoke_mandate` revoked a mandate. topic: ("revoke",) data: mandate_id
pub fn mandate_revoked(env: &Env, mandate_id: &BytesN<32>) {
    env.events()
        .publish((symbol_short!("revoke"),), mandate_id.clone());
}

/// `register_pool` stored a pool. topic: ("pool_reg", originator)
/// data: (pool_id, merchant, asset, threshold_qty, threshold_value, clearing_deadline)
#[allow(clippy::too_many_arguments)]
pub fn pool_registered(
    env: &Env,
    pool_id: &BytesN<32>,
    originator: &Address,
    merchant: &Address,
    asset: &Address,
    threshold_qty: u128,
    threshold_value: u128,
    clearing_deadline: u64,
) {
    env.events().publish(
        (symbol_short!("pool_reg"), originator.clone()),
        (
            pool_id.clone(),
            merchant.clone(),
            asset.clone(),
            threshold_qty,
            threshold_value,
            clearing_deadline,
        ),
    );
}

/// `commit_child` linked a child. topic: ("child_com", pool_id) data: (mandate_id, worst_case)
pub fn child_committed(env: &Env, pool_id: &BytesN<32>, mandate_id: &BytesN<32>, worst_case: i128) {
    env.events().publish(
        (symbol_short!("child_com"), pool_id.clone()),
        (mandate_id.clone(), worst_case),
    );
}

/// A child left the pool (revoke, evict, abort, zero-alloc, excluded).
/// topic: ("child_rel", pool_id) data: mandate_id
pub fn child_released(env: &Env, pool_id: &BytesN<32>, mandate_id: &BytesN<32>) {
    env.events().publish(
        (symbol_short!("child_rel"), pool_id.clone()),
        mandate_id.clone(),
    );
}

/// `clear_pool` captured. topic: ("pool_clr", pool_id)
/// data: (clearing_price, allocation_root, net_value, total_fee)
pub fn pool_cleared(
    env: &Env,
    pool_id: &BytesN<32>,
    clearing_price: i128,
    allocation_root: &BytesN<32>,
    net_value: i128,
    total_fee: i128,
) {
    env.events().publish(
        (symbol_short!("pool_clr"), pool_id.clone()),
        (
            clearing_price,
            allocation_root.clone(),
            net_value,
            total_fee,
        ),
    );
}

/// `clear_pool` aborted (predicate unmet at close, or past the capture
/// window). topic: ("pool_abrt", pool_id) data: ()
pub fn pool_aborted(env: &Env, pool_id: &BytesN<32>) {
    env.events()
        .publish((symbol_short!("pool_abrt"), pool_id.clone()), ());
}
