//! Reentrancy regression test — a malicious SEP-41 asset reenters
//! `execute_payment` during `transfer_from`. The replay guard (`expected_seq`)
//! plus checks-effects-interactions ordering (state persisted before the
//! external call) prevent any double-spend: `spent`/`seq` advance exactly once.
#![cfg(test)]

use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

use crate::{MandateRegistry, MandateRegistryClient};

// A malicious "token" that, on transfer_from, reenters execute_payment.
#[contract]
pub struct EvilToken;

#[contractimpl]
impl EvilToken {
    pub fn set(env: Env, registry: Address, id: BytesN<32>, amount: i128) {
        env.storage().instance().set(&0u32, &registry);
        env.storage().instance().set(&1u32, &id);
        env.storage().instance().set(&2u32, &amount);
    }

    // SEP-41 surface used by the contract.
    pub fn transfer_from(env: Env, _spender: Address, _from: Address, _to: Address, _amount: i128) {
        let registry: Address = env.storage().instance().get(&0u32).unwrap();
        let id: BytesN<32> = env.storage().instance().get(&1u32).unwrap();
        let amount: i128 = env.storage().instance().get(&2u32).unwrap();
        let c = MandateRegistryClient::new(&env, &registry);
        // Reenter with the *advanced* seq (1) — a "valid" follow-on seq.
        let _ = c.try_execute_payment(&id, &amount, &1u32);
    }
    // Other methods the contract never calls; provide a balance for completeness.
    pub fn balance(_env: Env, _id: Address) -> i128 {
        0
    }
}

#[test]
fn reentrancy_via_evil_token() {
    let env = Env::default();
    env.mock_all_auths(); // most permissive: even THIS still must respect the contract logic
    env.ledger().set_timestamp(1_000);

    let registry = env.register(MandateRegistry, ());
    let evil = env.register(EvilToken, ());

    let user = Address::generate(&env);
    let agent = Address::generate(&env);
    let merchant = Address::generate(&env);

    let id = BytesN::from_array(&env, &[7u8; 32]);
    let client = MandateRegistryClient::new(&env, &registry);
    client.register_mandate(
        &user,
        &agent,
        &merchant,
        &evil,
        &50_000_000i128,
        &10_000u64,
        &id,
        &None,
        &soroban_sdk::Vec::new(&env),
    );

    // Configure evil token to reenter.
    EvilTokenClient::new(&env, &evil).set(&registry, &id, &10_000_000i128);

    // Outer call: seq 0. Inner reentry tries seq 1.
    client.execute_payment(&id, &10_000_000i128, &0u32);

    let m = client.get_mandate(&id);
    // If reentry succeeded (double-spend), spent==2*SPEND, seq==2.
    // If the seq guard + nested-auth blocks it, spent==SPEND, seq==1.
    // Panic encodes the observed values into the failure message either way.
    assert_eq!(
        (m.spent, m.seq),
        (10_000_000i128, 1u32),
        "REENTRY OBSERVED: spent/seq differ from single-spend baseline"
    );
}
