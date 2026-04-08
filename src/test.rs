#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger, LedgerInfo},
        token, Address, BytesN, Env,
    };

    // ── Helper: deploy a mock USDC token and mint to admin ──────────────────

    fn setup() -> (Env, Address, Address, Address, i128, u64) {
        let env        = Env::default();
        env.mock_all_auths();

        let admin      = Address::generate(&env);
        let scholar    = Address::generate(&env);
        let amount: i128 = 15_000_0000000; // 15,000 USDC in stroops
        let deadline: u64 = env.ledger().timestamp() + 86400; // 24h from now

        // Deploy the mock USDC token contract
        let usdc_id    = env.register_stellar_asset_contract(admin.clone());
        let usdc_admin = token::StellarAssetClient::new(&env, &usdc_id);

        // Mint USDC to admin so they can fund the scholarship escrow
        usdc_admin.mint(&admin, &amount);

        (env, admin, scholar, usdc_id, amount, deadline)
    }

    // ── Test 1: Happy path — scholar submits proof and receives funds ────────

    #[test]
    fn test_happy_path_proof_and_release() {
        let (env, admin, scholar, usdc_id, amount, deadline) = setup();

        let contract_id = env.register_contract(None, ScholarChain);
        let client      = ScholarChainClient::new(&env, &contract_id);

        // NGO initializes the scholarship and funds the escrow
        client.initialize(&admin, &scholar, &usdc_id, &amount, &deadline);

        // Scholar submits a SHA-256 hash of their enrollment certificate
        let proof_hash  = BytesN::from_array(&env, &[1u8; 32]);
        client.submit_proof(&scholar, &proof_hash);

        // Scholar's wallet should now hold the USDC
        let usdc_client = token::Client::new(&env, &usdc_id);
        assert_eq!(usdc_client.balance(&scholar), amount);

        // Contract should be marked as released
        assert!(client.is_released());
    }

    // ── Test 2: Edge case — wrong wallet cannot submit proof ────────────────

    #[test]
    #[should_panic(expected = "unauthorized: not the registered scholar")]
    fn test_wrong_scholar_cannot_submit_proof() {
        let (env, admin, scholar, usdc_id, amount, deadline) = setup();

        let contract_id  = env.register_contract(None, ScholarChain);
        let client       = ScholarChainClient::new(&env, &contract_id);

        client.initialize(&admin, &scholar, &usdc_id, &amount, &deadline);

        // An imposter tries to claim the scholarship
        let imposter    = Address::generate(&env);
        let proof_hash  = BytesN::from_array(&env, &[2u8; 32]);

        client.submit_proof(&imposter, &proof_hash); // must panic
    }

    // ── Test 3: State verification — proof hash is stored on-chain ──────────

    #[test]
    fn test_proof_hash_stored_correctly() {
        let (env, admin, scholar, usdc_id, amount, deadline) = setup();

        let contract_id = env.register_contract(None, ScholarChain);
        let client      = ScholarChainClient::new(&env, &contract_id);

        client.initialize(&admin, &scholar, &usdc_id, &amount, &deadline);

        let proof_hash  = BytesN::from_array(&env, &[42u8; 32]);
        client.submit_proof(&scholar, &proof_hash);

        // The exact proof hash must be retrievable from contract storage
        let stored = client.get_proof_hash().expect("proof hash should exist");
        assert_eq!(stored, proof_hash);
    }

    // ── Test 4: Clawback — NGO recovers funds after deadline passes ──────────

    #[test]
    fn test_clawback_after_deadline() {
        let (env, admin, scholar, usdc_id, amount, deadline) = setup();

        let contract_id = env.register_contract(None, ScholarChain);
        let client      = ScholarChainClient::new(&env, &contract_id);

        client.initialize(&admin, &scholar, &usdc_id, &amount, &deadline);

        // Fast-forward ledger time past the deadline
        env.ledger().set(LedgerInfo {
            timestamp:          deadline + 1,
            protocol_version:   20,
            sequence_number:    100,
            network_id:         Default::default(),
            base_reserve:       10,
            min_temp_entry_ttl: 16,
            min_persistent_entry_ttl: 100_000,
            max_entry_ttl:      10_000_000,
        });

        // NGO claws back unreleased funds
        client.clawback(&admin);

        let usdc_client = token::Client::new(&env, &usdc_id);
        assert_eq!(usdc_client.balance(&admin), amount);
        assert!(client.is_released());
    }

    // ── Test 5: Double release blocked — cannot submit proof after release ───

    #[test]
    #[should_panic(expected = "funds already disbursed or clawed back")]
    fn test_cannot_submit_proof_after_release() {
        let (env, admin, scholar, usdc_id, amount, deadline) = setup();

        let contract_id = env.register_contract(None, ScholarChain);
        let client      = ScholarChainClient::new(&env, &contract_id);

        client.initialize(&admin, &scholar, &usdc_id, &amount, &deadline);

        let proof_hash = BytesN::from_array(&env, &[7u8; 32]);

        // First submission succeeds
        client.submit_proof(&scholar, &proof_hash);

        // Second submission must panic — funds already gone
        client.submit_proof(&scholar, &proof_hash);
    }
}
