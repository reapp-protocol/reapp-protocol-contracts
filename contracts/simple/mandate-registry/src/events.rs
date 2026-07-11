//! Emitted events. Leaf module — anyone may emit.

use soroban_sdk::{symbol_short, Address, BytesN, Env};

/// The operational administrator was rotated.
/// topic: ("admin",) data: new_admin
pub fn admin_set(env: &Env, new_admin: &Address) {
    env.events()
        .publish((symbol_short!("admin"),), new_admin.clone());
}

/// The money path was stopped. topic: ("paused", admin) data: ()
pub fn paused(env: &Env, admin: &Address) {
    env.events()
        .publish((symbol_short!("paused"), admin.clone()), ());
}

/// The money path was restored. topic: ("unpaused", admin) data: ()
pub fn unpaused(env: &Env, admin: &Address) {
    env.events()
        .publish((symbol_short!("unpaused"), admin.clone()), ());
}

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
