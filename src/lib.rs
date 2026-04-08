#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short,
    Address, BytesN, Env, Symbol,
};

// ─── Storage Keys ───────────────────────────────────────────────────────────

const ADMIN: Symbol         = symbol_short!("ADMIN");
const SCHOLAR: Symbol       = symbol_short!("SCHOLAR");
const AMOUNT: Symbol        = symbol_short!("AMOUNT");
const DEADLINE: Symbol      = symbol_short!("DEADLINE");
const PROOF_HASH: Symbol    = symbol_short!("PROOF");
const RELEASED: Symbol      = symbol_short!("RELEASED");
const USDC_TOKEN: Symbol    = symbol_short!("USDC");

// ─── Data Types ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ScholarStatus {
    Pending,    // Funds locked, waiting for enrollment proof
    Verified,   // Proof submitted and verified, funds released
    Clawedback, // Scholar dropped out, funds returned to NGO
}

#[contracttype]
#[derive(Clone)]
pub struct ScholarRecord {
    pub scholar:    Address,
    pub amount:     i128,
    pub deadline:   u64,
    pub proof_hash: Option<BytesN<32>>,
    pub status:     ScholarStatus,
}

// ─── Contract ───────────────────────────────────────────────────────────────

#[contract]
pub struct ScholarChain;

#[contractimpl]
impl ScholarChain {

    /// Called once by the NGO to initialize the scholarship contract.
    /// Stores the scholar wallet, USDC amount, enrollment deadline,
    /// and the USDC token contract address. Marks status as Pending.
    pub fn initialize(
        env:        Env,
        admin:      Address,      // NGO wallet — must sign this tx
        scholar:    Address,      // Scholar's Stellar wallet
        usdc_token: Address,      // Stellar USDC contract address
        amount:     i128,         // Amount in stroops (USDC smallest unit)
        deadline:   u64,          // Unix timestamp: enrollment proof must arrive before this
    ) {
        // Prevent re-initialization
        if env.storage().instance().has(&ADMIN) {
            panic!("already initialized");
        }

        admin.require_auth();

        // Persist all scholarship parameters
        env.storage().instance().set(&ADMIN,      &admin);
        env.storage().instance().set(&SCHOLAR,    &scholar);
        env.storage().instance().set(&USDC_TOKEN, &usdc_token);
        env.storage().instance().set(&AMOUNT,     &amount);
        env.storage().instance().set(&DEADLINE,   &deadline);
        env.storage().instance().set(&RELEASED,   &false);

        // Transfer USDC from NGO into this contract (escrow)
        let token_client = soroban_sdk::token::Client::new(&env, &usdc_token);
        token_client.transfer(&admin, &env.current_contract_address(), &amount);
    }

    /// Called by the scholar to submit their enrollment proof.
    /// The proof is a SHA-256 hash of their enrollment certificate,
    /// computed client-side and submitted here for on-chain storage.
    /// Once submitted before the deadline, funds are released immediately.
    pub fn submit_proof(
        env:        Env,
        scholar:    Address,      // Must match the registered scholar
        proof_hash: BytesN<32>,   // SHA-256 hash of enrollment document
    ) {
        scholar.require_auth();

        // Only the registered scholar can submit proof
        let registered: Address = env.storage().instance().get(&SCHOLAR).unwrap();
        if scholar != registered {
            panic!("unauthorized: not the registered scholar");
        }

        // Funds must not already be released or clawed back
        let released: bool = env.storage().instance().get(&RELEASED).unwrap();
        if released {
            panic!("funds already disbursed or clawed back");
        }

        // Proof must arrive before the enrollment deadline
        let deadline: u64 = env.storage().instance().get(&DEADLINE).unwrap();
        if env.ledger().timestamp() > deadline {
            panic!("enrollment deadline passed");
        }

        // Store the proof hash on-chain (immutable record)
        env.storage().instance().set(&PROOF_HASH, &proof_hash);

        // Release funds to scholar
        let usdc_token: Address = env.storage().instance().get(&USDC_TOKEN).unwrap();
        let amount: i128        = env.storage().instance().get(&AMOUNT).unwrap();

        let token_client = soroban_sdk::token::Client::new(&env, &usdc_token);
        token_client.transfer(&env.current_contract_address(), &scholar, &amount);

        // Mark as released so clawback and re-submission are blocked
        env.storage().instance().set(&RELEASED, &true);
    }

    /// Called by the NGO admin if the scholar fails to submit proof
    /// before the deadline (e.g. dropped out, missed enrollment).
    /// Returns escrowed USDC back to the NGO wallet.
    pub fn clawback(env: Env, admin: Address) {
        admin.require_auth();

        // Only the original NGO admin can trigger clawback
        let registered_admin: Address = env.storage().instance().get(&ADMIN).unwrap();
        if admin != registered_admin {
            panic!("unauthorized: not the NGO admin");
        }

        // Cannot claw back if funds were already released to scholar
        let released: bool = env.storage().instance().get(&RELEASED).unwrap();
        if released {
            panic!("funds already disbursed to scholar");
        }

        // Clawback is only valid after the deadline has passed
        let deadline: u64 = env.storage().instance().get(&DEADLINE).unwrap();
        if env.ledger().timestamp() <= deadline {
            panic!("deadline has not passed yet");
        }

        // Return escrowed USDC to NGO
        let usdc_token: Address = env.storage().instance().get(&USDC_TOKEN).unwrap();
        let amount: i128        = env.storage().instance().get(&AMOUNT).unwrap();

        let token_client = soroban_sdk::token::Client::new(&env, &usdc_token);
        token_client.transfer(&env.current_contract_address(), &admin, &amount);

        env.storage().instance().set(&RELEASED, &true);
    }

    /// Read-only view: returns the scholar's wallet address.
    pub fn get_scholar(env: Env) -> Address {
        env.storage().instance().get(&SCHOLAR).unwrap()
    }

    /// Read-only view: returns the escrowed USDC amount.
    pub fn get_amount(env: Env) -> i128 {
        env.storage().instance().get(&AMOUNT).unwrap()
    }

    /// Read-only view: returns the enrollment deadline timestamp.
    pub fn get_deadline(env: Env) -> u64 {
        env.storage().instance().get(&DEADLINE).unwrap()
    }

    /// Read-only view: returns whether funds have been released/clawed back.
    pub fn is_released(env: Env) -> bool {
        env.storage().instance().get(&RELEASED).unwrap_or(false)
    }

    /// Read-only view: returns the stored proof hash if submitted.
    pub fn get_proof_hash(env: Env) -> Option<BytesN<32>> {
        env.storage().instance().get(&PROOF_HASH)
    }
}
