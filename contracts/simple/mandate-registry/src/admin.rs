//! Minimal administration plane: authority rotation, emergency pause, and
//! same-address WASM upgrades. All state access remains centralized in
//! `storage`; Soroban host authorization rejects every unauthorized call.

use soroban_sdk::{Address, BytesN, Env};

use crate::{events, storage};

pub fn get_admin(env: &Env) -> Address {
    storage::get_admin(env)
}

pub fn set_admin(env: &Env, new_admin: Address) {
    let current = storage::get_admin(env);
    current.require_auth();
    storage::set_admin(env, &new_admin);
    events::admin_set(env, &new_admin);
}

pub fn is_paused(env: &Env) -> bool {
    storage::is_paused(env)
}

pub fn pause(env: &Env) {
    let admin = storage::get_admin(env);
    admin.require_auth();
    if !storage::is_paused(env) {
        storage::set_paused(env, true);
        events::paused(env, &admin);
    }
}

pub fn unpause(env: &Env) {
    let admin = storage::get_admin(env);
    admin.require_auth();
    if storage::is_paused(env) {
        storage::set_paused(env, false);
        events::unpaused(env, &admin);
    }
}

pub fn upgrade(env: &Env, new_wasm_hash: BytesN<32>) {
    storage::get_admin(env).require_auth();
    env.deployer().update_current_contract_wasm(new_wasm_hash);
}
