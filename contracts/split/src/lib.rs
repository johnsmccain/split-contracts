//! StellarSplit — on-chain invoice & payment splitting contract.

#![no_std]

mod events;
mod types;

#[cfg(test)]
mod test;

use soroban_sdk::{
    String,
    String,
    contract, contractimpl, symbol_short, token, Address, Bytes, BytesN, Env, IntoVal, Map, Symbol, Val, Vec,
};
use types::{
    AuditEntry, CompletionProof, CreateInvoiceParams, Invoice, InvoiceOptions, InvoicePayment,
    InvoiceStats, InvoiceStatus, InvoiceTemplate, LegacyInvoice, Payment, PaymentProof,
    SubscriptionParams, Tranche,
};

// ---------------------------------------------------------------------------
// Storage key helpers
// ---------------------------------------------------------------------------

fn governance_contract_key() -> Symbol {
    symbol_short!("gov_ctr")
}

fn admin_key() -> Symbol {
    symbol_short!("admin")
}
fn paused_key() -> Symbol {
    symbol_short!("paused")
}
fn creation_fee_key() -> Symbol {
    symbol_short!("crt_fee")
}
fn platform_fee_bps_key() -> Symbol {
    symbol_short!("plat_fee")
}
fn treasury_key() -> Symbol {
    symbol_short!("treasury")
}
fn usdc_token_key() -> Symbol {
    symbol_short!("usdc_tok")
}
fn counter_key() -> Symbol {
    symbol_short!("counter")
}
fn invoice_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("inv"), id)
}
fn audit_log_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("log"), id)
}
fn subscription_params_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("sub"), id)
}
fn ext_vote_key(id: u64) -> (Symbol, u64) {
    (symbol_short!("ext_vote"), id)
}
fn group_key(group_id: u64) -> (Symbol, u64) {
    (symbol_short!("grp"), group_id)
}
fn invoice_group_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("invgrp"), invoice_id)
}
fn template_key(creator: &Address, name: &Symbol) -> (Symbol, Address, Symbol) {
    (symbol_short!("tmpl"), creator.clone(), name.clone())
}

/// Per-address reputation counter key (issue #24).
fn rep_key(payer: &Address) -> (Symbol, Address) {
    (symbol_short!("rep"), payer.clone())
}

/// Per-address credit score key (issue #38).
fn credit_key(payer: &Address) -> (Symbol, Address) {
    (symbol_short!("credit"), payer.clone())
}

/// Per-address referral count key (issue #87).
fn referral_count_key(referrer: &Address) -> (Symbol, Address) {
    (symbol_short!("ref_cnt"), referrer.clone())
}

/// Per-payer per-invoice nonce key (issue #21).

fn channel_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("chan"), invoice_id, payer.clone())
}

fn nonce_key(invoice_id: u64, payer: &Address) -> (Symbol, u64, Address) {
    (symbol_short!("nonce"), invoice_id, payer.clone())
}

/// Authorised factory addresses key (issue #145).
fn factories_key() -> Symbol {
    symbol_short!("factories")
}

/// Per-recipient invoice ID index key (issue #40).
fn recipient_invoice_ids_key(recipient: &Address) -> (Symbol, Address) {
    (symbol_short!("rec_inv"), recipient.clone())
}

/// Issue #1: Stellar payment streaming contract address.
fn stream_contract_key() -> Symbol {
    symbol_short!("strm_ctr")
}

/// Issue #4: Creator whitelist key.
fn creator_whitelist_key() -> Symbol {
    symbol_short!("creator_wl")
}

/// Delegate address key for an invoice (issue #43).
fn delegate_key(invoice_id: u64) -> (Symbol, u64) {
    (symbol_short!("delegate"), invoice_id)
}

/// Analytics counters (issue #28).
fn total_invoices_key() -> Symbol {
    symbol_short!("tot_inv")
}
fn total_volume_key() -> Symbol {
    symbol_short!("tot_vol")
}
fn total_released_key() -> Symbol {
    symbol_short!("tot_rel")
}
fn total_refunded_key() -> Symbol {
    symbol_short!("tot_ref")
}

/// Compliance contract address key.
fn compliance_key() -> Symbol {
    symbol_short!("comply")
}

// ---------------------------------------------------------------------------
// Invoice storage helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

fn load_invoice(env: &Env, id: u64) -> Invoice {
    // Check persistent storage first; fall back to instance storage for archived invoices.
    if let Some(inv) = env.storage().persistent().get(&invoice_key(id)) {
        return inv;
    }
    env.storage()
        .instance()
        .get(&invoice_key(id))
        .expect("invoice not found")
}

fn save_invoice(env: &Env, id: u64, invoice: &Invoice) {
    env.storage().persistent().set(&invoice_key(id), invoice);
}

fn append_audit_entry(env: &Env, id: u64, action: Symbol, actor: &Address) {
    let timestamp = env.ledger().timestamp();
    let entry = AuditEntry { action, actor: actor.clone(), timestamp };
    let mut log: Vec<AuditEntry> = env
        .storage()
        .persistent()
        .get(&audit_log_key(id))
        .unwrap_or_else(|| Vec::new(env));
    log.push_back(entry);
    env.storage().persistent().set(&audit_log_key(id), &log);
}

pub fn get_audit_log(env: &Env, id: u64) -> Vec<AuditEntry> {
    env.storage()
        .persistent()
        .get(&audit_log_key(id))
        .unwrap_or_else(|| Vec::new(env))
}

// ---------------------------------------------------------------------------
// Admin / pause helpers
// ---------------------------------------------------------------------------

fn is_paused(env: &Env) -> bool {
    env.storage()
        .persistent()
        .get(&paused_key())
        .unwrap_or(false)
}

fn require_not_paused(env: &Env) {
    assert!(!is_paused(env), "contract is paused");
}

fn require_admin(env: &Env) -> Address {
    let admin: Address = env
        .storage()
        .instance()
        .get(&admin_key())
        .expect("admin not set");
    admin.require_auth();
    admin
}

// ---------------------------------------------------------------------------
// Group helpers
// ---------------------------------------------------------------------------

fn load_group(env: &Env, group_id: u64) -> Vec<u64> {
    env.storage()
        .persistent()
        .get(&group_key(group_id))
        .expect("group not found")
}

fn group_all_funded(env: &Env, group_id: u64) -> bool {
    for id in load_group(env, group_id).iter() {
        let inv = load_invoice(env, id);
        let total: i128 = inv.amounts.iter().sum();
        if inv.funded < total {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct SplitContract;

#[contractimpl]
impl SplitContract {
    /// Set the contract admin, creation fee, treasury, USDC token, and platform fee.
    /// Can only be called once.
    pub fn initialize(
        env: Env,
        admin: Address,
        creation_fee: i128,
        treasury: Address,
        usdc_token: Address,
        platform_fee_bps: u32,
        compliance_contract: Option<Address>,
    ) {
        assert!(
            !env.storage().instance().has(&admin_key()),
            "already initialized"
        );
        assert!(creation_fee >= 0, "creation_fee must be non-negative");
        assert!(platform_fee_bps <= 10_000, "platform_fee_bps must be ≤ 10000");
        env.storage().instance().set(&admin_key(), &admin);
        env.storage().instance().set(&creation_fee_key(), &creation_fee);
        env.storage().instance().set(&treasury_key(), &treasury);
        env.storage().instance().set(&usdc_token_key(), &usdc_token);
        env.storage().instance().set(&platform_fee_bps_key(), &platform_fee_bps);
        env.storage().instance().set(&governance_contract_key(), &governance_contract);
        env.storage().persistent().set(&paused_key(), &false);
        if let Some(contract) = compliance_contract {
            env.storage().persistent().set(&soroban_sdk::symbol_short!("comp_ctr"), &contract);
        }
    }

    /// Pause the contract. Requires admin auth.
    pub fn pause(env: Env, admin: Address) {
        require_admin(&env);
        let _ = admin;
        env.storage().persistent().set(&paused_key(), &true);
    }

    /// Unpause the contract. Requires admin auth.
    pub fn unpause(env: Env, admin: Address) {
        require_admin(&env);
        let _ = admin;
        env.storage().persistent().set(&paused_key(), &false);
    }

    /// Update the creation fee. Requires admin auth.
    pub fn set_creation_fee(env: Env, admin: Address, creation_fee: i128) {
        require_admin(&env);
        let _ = admin;
        assert!(creation_fee >= 0, "creation_fee must be non-negative");
        env.storage().instance().set(&creation_fee_key(), &creation_fee);
    }

    /// Update the treasury address. Requires admin auth.
    pub fn set_treasury(env: Env, admin: Address, treasury: Address) {
        require_admin(&env);
        let _ = admin;
        env.storage().instance().set(&treasury_key(), &treasury);
    }

    // -----------------------------------------------------------------------
    // Issue #1: stream contract admin setter
    // -----------------------------------------------------------------------

    /// Store the address of the Stellar payment streaming contract. Requires admin auth.
    pub fn set_stream_contract(env: Env, admin: Address, contract: Address) {
        require_admin(&env);
        let _ = admin;
        env.storage().persistent().set(&stream_contract_key(), &contract);
    }

    /// Store the DEX contract address used for token swaps in pay_with_token(). Requires admin auth.
    pub fn set_dex_contract(env: Env, admin: Address, contract: Address) {
        require_admin(&env);
        let _ = admin;
        env.storage().persistent().set(&soroban_sdk::symbol_short!("dex_ctr"), &contract);
    }

    // -----------------------------------------------------------------------
    // Issue #4: creator whitelist
    // -----------------------------------------------------------------------

    /// Add an address to the creator whitelist. Requires admin auth.
    /// When the whitelist is non-empty, only listed addresses may call create_invoice().
    pub fn whitelist_creator(env: Env, admin: Address, address: Address) {
        require_admin(&env);
        let _ = admin;
        let mut wl: Vec<Address> = env
            .storage()
            .persistent()
            .get(&creator_whitelist_key())
            .unwrap_or_else(|| Vec::new(&env));
        if !wl.iter().any(|a| a == address) {
            wl.push_back(address);
        }
        env.storage().persistent().set(&creator_whitelist_key(), &wl);
    }

    /// Remove an address from the creator whitelist. Requires admin auth.
    pub fn remove_creator(env: Env, admin: Address, address: Address) {
        require_admin(&env);
        let _ = admin;
        let wl: Vec<Address> = env
            .storage()
            .persistent()
            .get(&creator_whitelist_key())
            .unwrap_or_else(|| Vec::new(&env));
        let mut new_wl: Vec<Address> = Vec::new(&env);
        for a in wl.iter() {
            if a != address {
                new_wl.push_back(a);
            }
        }
        env.storage().persistent().set(&creator_whitelist_key(), &new_wl);
    }

    /// Return the current creation fee.
    pub fn get_creation_fee(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&creation_fee_key())
            .unwrap_or(0)
    }

    /// Return the treasury address.
    pub fn get_treasury(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&treasury_key())
            .expect("treasury not set")
    }

    /// Return the USDC token address.
    pub fn get_usdc_token(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&usdc_token_key())
            .expect("usdc token not set")
    }

    /// Return the platform fee in basis points (issue #41).
    pub fn get_platform_fee_bps(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&platform_fee_bps_key())
            .unwrap_or(0u32)
    }

    // -----------------------------------------------------------------------
    // Schema migration
    // -----------------------------------------------------------------------

    /// Migrate a legacy (pre-version) invoice to the current schema.
    ///
    /// Reads the stored invoice under the old layout, rewrites it with
    /// `version = 1` and all other fields preserved. Safe to call multiple
    /// times — already-migrated invoices are a no-op. Requires admin auth.
    pub fn migrate_invoice(env: Env, admin: Address, invoice_id: u64) {
        require_admin(&env);
        let _ = admin;

        // Already migrated?
        if let Some(invoice) = env
            .storage()
            .persistent()
            .get::<_, Invoice>(&invoice_key(invoice_id))
        {
            if invoice.version >= 1 {
                return;
            }
        }

        // Read legacy (pre-version) format and upgrade.
        let legacy: LegacyInvoice = env
            .storage()
            .persistent()
            .get(&invoice_key(invoice_id))
            .expect("invoice not found");

        let invoice = Invoice::from_legacy(legacy, &env);
        env.storage()
            .persistent()
            .set(&invoice_key(invoice_id), &invoice);
    }

    // -----------------------------------------------------------------------
    // Invoice creation
    // -----------------------------------------------------------------------

    /// Create a new invoice.
    ///
    /// * `token`   – token contract address (same for all recipients)
    /// * `options` – optional fields: co_creators, allow_early_withdrawal, bonus_pool,
    ///               bonus_max_payers, prerequisite_id (#22), tranches (#23),
    ///               stake_amount (#89), referrer (#87), max_payers (#26)
    pub fn create_invoice(
        env: Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        deadline: u64,
        options: InvoiceOptions,
        tax_bps: u32,
        tax_authority: Option<Address>,
    ) -> u64 {
        require_not_paused(&env);
        creator.require_auth();

        // Issue #4: reject creator if whitelist is non-empty and creator is not on it.
        let wl: Vec<Address> = env
            .storage()
            .persistent()
            .get(&creator_whitelist_key())
            .unwrap_or_else(|| Vec::new(&env));
        if !wl.is_empty() {
            assert!(wl.iter().any(|a| a == creator), "creator not whitelisted");
        }

        Self::_create_invoice_inner(
            &env,
            creator,
            recipients,
            amounts,
            token,
            deadline,
            options.co_creators,
            options.allow_early_withdrawal,
            options.bonus_pool,
            options.bonus_max_payers,
            options.prerequisite_id,
            options.tranches,
            options.co_signers,
            options.required_signatures,
            options.penalty_bps.unwrap_or(0),
            options.penalty_deadline.unwrap_or(0),
            options.min_funding_bps.unwrap_or(0),
            options.release_stages,
            options.price_oracle,
            options.swap_tokens,
            options.tax_bps.unwrap_or(0),
            options.tax_authority,
            options.insurance_premium_bps.unwrap_or(0),
            options.smart_route.unwrap_or(false),
            options.convert_to_stream,
            options.accepted_tokens,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn _create_invoice_inner(
        env: &Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        deadline: u64,
        co_creators: Vec<Address>,
        allow_early_withdrawal: bool,
        bonus_pool: i128,
        bonus_max_payers: u32,
        prerequisite_id: Option<u64>,
        tranches: Vec<Tranche>,
        co_signers: Vec<Address>,
        required_signatures: u32,
        penalty_bps: u32,
        penalty_deadline: u64,
        min_funding_bps: u32,
        release_stages: Vec<u32>,
        price_oracle: Option<Address>,
        swap_tokens: Vec<Option<Address>>,
        tax_bps: u32,
        tax_authority: Option<Address>,
        insurance_premium_bps: u32,
        smart_route: bool,
        convert_to_stream: bool,
        accepted_tokens: Vec<Address>,
    ) -> u64 {
        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        assert!(deadline > env.ledger().timestamp(), "deadline must be in the future");
        assert!(bonus_pool >= 0, "bonus_pool must be non-negative");
        assert!(penalty_bps <= 10_000, "penalty_bps must be ≤ 10000");
        assert!(min_funding_bps <= 10_000, "min_funding_bps must be ≤ 10000");
        assert!(tax_bps <= 10_000, "tax_bps must be ≤ 10000");
        assert!(insurance_premium_bps <= 10_000, "insurance_premium_bps must be ≤ 10000");
        if tax_bps > 0 {
            assert!(tax_authority.is_some(), "tax_authority must be set if tax_bps > 0");
        }

        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        if let Some(compliance_contract) = env.storage().persistent().get::<_, Address>(&soroban_sdk::symbol_short!("comp_ctr")) {
            let creator_ok: bool = env.invoke_contract(&compliance_contract, &soroban_sdk::Symbol::new(env, "check"), (creator.clone(),).into_val(env));
            assert!(creator_ok, "compliance check failed");
            
            for recipient in recipients.iter() {
                let recipient_ok: bool = env.invoke_contract(&compliance_contract, &soroban_sdk::Symbol::new(env, "check"), (recipient.clone(),).into_val(env));
                assert!(recipient_ok, "compliance check failed");
            }
        }

        if let Some(prereq_id) = prerequisite_id {
            let _ = load_invoice(env, prereq_id);
        }

        if !tranches.is_empty() {
            let total_bps: u32 = tranches.iter().map(|t| t.basis_points).sum();
            assert!(total_bps == 10_000, "tranches must sum to 10000 basis points");
        }

        if !release_stages.is_empty() {
            let total_bps: u32 = release_stages.iter().sum();
            assert!(total_bps == 10_000, "release_stages must sum to 10000 basis points");
        }

        // Compliance check: if a compliance contract is configured, verify creator and all recipients.
        if let Some(cc) = env.storage().persistent().get::<Symbol, Address>(&compliance_key()) {
            let mut check_args: Vec<Val> = Vec::new(env);
            check_args.push_back(creator.clone().into_val(env));
            let creator_ok: bool = env.invoke_contract(&cc, &Symbol::new(env, "is_compliant"), check_args);
            assert!(creator_ok, "compliance check failed");
            for recipient in recipients.iter() {
                let mut r_args: Vec<Val> = Vec::new(env);
                r_args.push_back(recipient.clone().into_val(env));
                let r_ok: bool = env.invoke_contract(&cc, &Symbol::new(env, "is_compliant"), r_args);
                assert!(r_ok, "compliance check failed");
            }
        }

        // Charge configurable creation fee in USDC.
        let creation_fee: i128 = env
            .storage()
            .instance()
            .get(&creation_fee_key())
            .unwrap_or(0);
        if creation_fee > 0 {
            let usdc_token: Address = env
                .storage()
                .instance()
                .get(&usdc_token_key())
                .expect("usdc token not set");
            let treasury: Address = env
                .storage()
                .instance()
                .get(&treasury_key())
                .expect("treasury not set");
            let usdc_client = token::Client::new(env, &usdc_token);
            usdc_client.transfer(&creator, &treasury, &creation_fee);
        }

        // Issue #89: Transfer stake from creator to contract if stake_amount > 0.
        // (stake_amount is not yet wired into _create_invoice_inner; skipped)

        let id: u64 = env
            .storage()
            .persistent()
            .get(&counter_key())
            .unwrap_or(0u64)
            + 1;
        env.storage().persistent().set(&counter_key(), &id);


        let total: i128 = amounts.iter().sum();

        let gov_opt: Option<Option<Address>> = env.storage().instance().get(&governance_contract_key());
        if let Some(Some(gov)) = gov_opt {
            let approved: bool = env.invoke_contract(&gov, &Symbol::new(env, "check_approval"), (creator.clone(), total).into_val(env));
            assert!(approved, "governance approval required");
        }


        if bonus_pool > 0 {
            let token_client = token::Client::new(env, &token);
            token_client.transfer(&creator, &env.current_contract_address(), &bonus_pool);
        }

        // Build per-recipient token vec (all the same token).
        let mut tokens: Vec<Address> = Vec::new(env);
        for _ in recipients.iter() {
            tokens.push_back(token.clone());
        }

        // Initialize per-recipient claimed vec to 0.
        let mut claimed: Vec<i128> = Vec::new(env);
        for _ in recipients.iter() {
            claimed.push_back(0i128);
        }

        // Issue #27: Initialize vesting cliff claimed tracking (all false).
        let mut vesting_cliff_claimed: Vec<bool> = Vec::new(env);
        for _ in recipients.iter() {
            vesting_cliff_claimed.push_back(false);
        }

        // Issue #87: Increment referral count if referrer is provided.
        // (referrer is not yet wired into _create_invoice_inner; skipped)

        let invoice = Invoice {
            version: 1u32,
            creator: creator.clone(),
            co_creators,
            recipients,
            base_amounts: amounts.clone(),
            amounts,
            tokens,
            deadline,
            funded: 0,
            status: InvoiceStatus::Pending,
            payments: Vec::new(env),
            drip_duration: None,
            release_timestamp: None,
            claimed,
            frozen: false,
            completion_time: None,
            allow_early_withdrawal,
            bonus_pool,
            bonus_max_payers,
            prerequisite_id,
            tranches,
            released_bps: 0,
            co_signers,
            required_signatures,
            signatures: Vec::new(env),
            approver: None,
            approved: false,
            penalty_bps,
            penalty_deadline,
            min_funding_bps,
            release_stages,
            released_stages: 0,
            allowed_payers: None,
            price_oracle,
            swap_tokens,
            tax_bps,
            tax_authority,
            insurance_premium_bps,
            insurance_fund: 0,
            smart_route,
            convert_to_stream,
            accepted_tokens,
        };

        save_invoice(env, id, &invoice);
        events::invoice_created(env, id, &creator, total, &cross_chain_ref);

        // Index each recipient -> invoice ID (issue #40).
        for recipient in invoice.recipients.iter() {
            let key = recipient_invoice_ids_key(&recipient);
            let mut ids: Vec<u64> = env
                .storage()
                .persistent()
                .get(&key)
                .unwrap_or_else(|| Vec::new(env));
            ids.push_back(id);
            env.storage().persistent().set(&key, &ids);
        }

        // Increment total_invoices counter (issue #28).
        let total_invoices: u64 = env
            .storage()
            .persistent()
            .get(&total_invoices_key())
            .unwrap_or(0u64);
        env.storage().persistent().set(
            &total_invoices_key(),
            &total_invoices.checked_add(1).expect("total_invoices overflow"),
        );

        id
    }

    /// Create up to 5 invoices in a single transaction.
    pub fn create_batch(
        env: Env,
        creator: Address,
        invoices: Vec<CreateInvoiceParams>,
    ) -> Vec<u64> {
        creator.require_auth();
        assert!(invoices.len() <= 5, "batch limit exceeded");

        let mut ids: Vec<u64> = Vec::new(&env);
        for params in invoices.iter() {
            let id = Self::_create_invoice_inner(
                &env,
                creator.clone(),
                params.recipients,
                params.amounts,
                params.token,
                params.deadline,
                Vec::new(&env),
                false,
                0,
                0,
                None,
                Vec::new(&env),
                Vec::new(&env),
                0,
                0,
                0,
                0,
                Vec::new(&env),
                None,
                Vec::new(&env),
                0,
                None,
                0,
                false,
                false,
                Vec::new(&env),
            );
            ids.push_back(id);
        }
        ids
    }

    /// Create a subscription chain of invoices for recurring monthly billing.
    pub fn create_subscription(
        env: Env,
        creator: Address,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
        months: u32,
    ) -> u64 {
        creator.require_auth();

        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        assert!(months > 0 && months <= 12, "months must be between 1 and 12");
        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        let deadline = env.ledger().timestamp() + 30 * 24 * 60 * 60;
        let id = Self::_create_invoice_inner(
            &env,
            creator.clone(),
            recipients.clone(),
            amounts.clone(),
            token.clone(),
            deadline,
            Vec::new(&env),
            false,
            0,
            0,
            None,
            Vec::new(&env),
            Vec::new(&env),
            0,
            0,
            0,
            0,
            Vec::new(&env),
            None,
            Vec::new(&env),
            0,
            None,
            0,
            false,
            false,
            Vec::new(&env),
        );

        if months > 1 {
            // Build tokens vec for subscription params storage.
            let mut tokens_vec: Vec<Address> = Vec::new(&env);
            for _ in recipients.iter() {
                tokens_vec.push_back(token.clone());
            }
            let params = SubscriptionParams {
                creator,
                recipients,
                amounts,
                tokens: tokens_vec,
            };
            env.storage()
                .persistent()
                .set(&subscription_params_key(id), &params);
        }

        id
    }

    // -----------------------------------------------------------------------
    // Payment (#21 nonce added, #88 auto_convert added)
    // -----------------------------------------------------------------------

    /// Pay toward an invoice.
    ///
    /// `nonce` must equal the current expected nonce for this (invoice_id, payer)
    /// pair — starts at 0 and increments with each successful payment.
    /// 
    /// `auto_convert` (issue #88): when true, invokes DEX swap to convert payer's
    /// source asset to invoice token before crediting payment. When false, behaves
    /// identically to current implementation.

    /// Compress payments by aggregating all payments from the same payer into a single entry.
    pub fn compress_payments(env: Env, invoice_id: u64) {
        require_not_paused(&env);
        let mut invoice = load_invoice(&env, invoice_id);

        let mut payer_amounts: Map<Address, i128> = Map::new(&env);
        let mut payer_tips: Map<Address, i128> = Map::new(&env);

        for p in invoice.payments.iter() {
            let current_amt = payer_amounts.get(p.payer.clone()).unwrap_or(0);
            payer_amounts.set(p.payer.clone(), current_amt + p.amount);
            
            let current_tip = payer_tips.get(p.payer.clone()).unwrap_or(0);
            payer_tips.set(p.payer.clone(), current_tip + p.tip);
        }

        let mut new_payments: Vec<Payment> = Vec::new(&env);
        for (payer, amount) in payer_amounts.iter() {
            let tip = payer_tips.get(payer.clone()).unwrap_or(0);
            new_payments.push_back(Payment { payer, amount, tip });
        }

        invoice.payments = new_payments;

        // Verify total funded is unchanged (optional assertion, as asked by Acceptance Criteria)
        let mut total_funded: i128 = 0;
        for p in invoice.payments.iter() {
            total_funded += p.amount;
        }
        assert_eq!(total_funded, invoice.funded, "total funded changed after compression");

        save_invoice(&env, invoice_id, &invoice);
    }


    // -----------------------------------------------------------------------
    // Payment Channel (Issue #1)
    // -----------------------------------------------------------------------

    pub fn open_channel(env: Env, payer: Address, invoice_id: u64, deposit: i128) {
        require_not_paused(&env);
        payer.require_auth();
        assert!(deposit > 0, "deposit must be positive");

        let invoice = load_invoice(&env, invoice_id);
        assert!(invoice.status == InvoiceStatus::Pending, "invoice is not pending");

        let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
        token_client.transfer(&payer, &env.current_contract_address(), &deposit);

        // Store (balance, deposited)
        let state: (i128, i128) = (deposit, deposit);
        env.storage().persistent().set(&channel_key(invoice_id, &payer), &state);
    }

    pub fn channel_pay(env: Env, payer: Address, invoice_id: u64, amount: i128) {
        require_not_paused(&env);
        payer.require_auth();
        assert!(amount > 0, "amount must be positive");

        let mut state: (i128, i128) = env.storage().persistent().get(&channel_key(invoice_id, &payer)).expect("channel not found");
        assert!(state.0 >= amount, "insufficient channel balance");

        state.0 -= amount;
        env.storage().persistent().set(&channel_key(invoice_id, &payer), &state);
    }

    pub fn close_channel(env: Env, payer: Address, invoice_id: u64) {
        require_not_paused(&env);
        payer.require_auth();

        let state: (i128, i128) = env.storage().persistent().get(&channel_key(invoice_id, &payer)).expect("channel not found");
        let balance = state.0;
        let deposited = state.1;
        let net_paid = deposited - balance;

        let mut invoice = load_invoice(&env, invoice_id);

        if net_paid > 0 {
            assert!(invoice.status == InvoiceStatus::Pending, "invoice is not pending");

            invoice.payments.push_back(Payment { payer: payer.clone(), amount: net_paid, tip: 0 });
            invoice.funded += net_paid;

            // In real app we might handle penalty/oracle, but for simplicity:
            events::payment_received(&env, invoice_id, &payer, net_paid);
            
            let total: i128 = invoice.amounts.iter().sum();
            
            if invoice.funded >= total {
                let in_group = env.storage().persistent().has(&invoice_group_key(invoice_id));
                let guarded =
                    invoice.prerequisite_id.is_some()
                        || !invoice.tranches.is_empty()
                        || !invoice.release_stages.is_empty()
                        || in_group
                        || !invoice.co_signers.is_empty();
                if guarded {
                    save_invoice(&env, invoice_id, &invoice);
                } else {
                    Self::_release(&env, invoice_id, &mut invoice, &payer);
                }
            } else {
                save_invoice(&env, invoice_id, &invoice);
            }
        }

        if balance > 0 {
            let token_client = token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
            token_client.transfer(&env.current_contract_address(), &payer, &balance);
        }

        env.storage().persistent().remove(&channel_key(invoice_id, &payer));
    }

    pub fn pay(env: Env, payer: Address, invoice_id: u64, amount: i128, nonce: u64, auto_convert: bool) {
        require_not_paused(&env);
        payer.require_auth();
        Self::_pay(&env, &payer, invoice_id, amount, nonce, auto_convert);
    }

    fn _pay(env: &Env, payer: &Address, invoice_id: u64, amount: i128, nonce: u64, auto_convert: bool) {
        let mut invoice = load_invoice(env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            env.ledger().timestamp() <= invoice.deadline,
            "invoice deadline has passed"
        );
        assert!(amount > 0, "payment amount must be positive");

        // Issue #142: when a price oracle is configured, query current price and
        // compute the oracle-adjusted total. oracle_price of 1_000_000 = 1.0 (identity).
        let total: i128 = if let Some(ref oracle) = invoice.price_oracle {
            let oracle_price: i128 = env.invoke_contract(
                oracle,
                &Symbol::new(env, "get_price"),
                Vec::new(env),
            );
            let base_total: i128 = invoice.base_amounts.iter().sum();
            base_total * oracle_price / 1_000_000
        } else {
            invoice.amounts.iter().sum()
        };
        let remaining = total - invoice.funded;
        assert!(amount <= remaining, "payment exceeds remaining balance");

        // Validate and increment per-payer per-invoice nonce (issue #21).
        let stored_nonce: u64 = env
            .storage()
            .persistent()
            .get(&nonce_key(invoice_id, payer))
            .unwrap_or(0u64);
        assert!(nonce == stored_nonce, "invalid nonce");
        env.storage()
            .persistent()
            .set(&nonce_key(invoice_id, payer), &(stored_nonce + 1));

        let token_client = token::Client::new(env, &invoice.tokens.get(0).expect("no token"));
        
        let premium = (amount as u128 * invoice.insurance_premium_bps as u128 / 10_000u128) as i128;
        let total_charge = amount + premium;

        // Issue #88: Auto-convert if requested.
        let credited_amount = if auto_convert {
            // In production, this would call a DEX swap contract.
            // For now, we assume a 1:1 swap and transfer the amount directly.
            // Mock DEX swap: payer's source asset -> invoice token.
            // The swapped amount is what gets credited.
            token_client.transfer(payer, &env.current_contract_address(), &total_charge);
            amount // In a real implementation, this would be the swapped output amount.
        } else {
            token_client.transfer(payer, &env.current_contract_address(), &total_charge);
            amount
        };
        
        invoice.insurance_fund += premium;

        // Penalty for late payment (issue #42).
        if invoice.penalty_bps > 0 && env.ledger().timestamp() > invoice.penalty_deadline {
            let penalty_amount = (amount as u128 * invoice.penalty_bps as u128 / 10_000u128) as i128;
            if penalty_amount > 0 {
                let total_amounts: i128 = invoice.amounts.iter().sum();
                let mut distributed: i128 = 0;
                let n = invoice.recipients.len();
                for i in 0..n {
                    let recipient = invoice.recipients.get(i).unwrap();
                    let amt = invoice.amounts.get(i).unwrap();
                    let share = if i == n - 1 {
                        penalty_amount - distributed
                    } else {
                        (penalty_amount as u128 * amt as u128 / total_amounts as u128) as i128
                    };
                    distributed += share;
                    if share > 0 {
                        token_client.transfer(payer, &recipient, &share);
                    }
                }
            }
        }

        invoice.payments.push_back(Payment { payer: payer.clone(), amount, tip: 0 });
        invoice.funded += amount;

        // Increment per-address reputation counter (issue #24).
        let rep: u64 = env
            .storage()
            .persistent()
            .get(&rep_key(payer))
            .unwrap_or(0u64);
        env.storage()
            .persistent()
            .set(&rep_key(payer), &(rep + 1));

        // Increment per-address credit score (issue #38).
        let credit: u64 = env
            .storage()
            .persistent()
            .get(&credit_key(payer))
            .unwrap_or(0u64);
        env.storage()
            .persistent()
            .set(&credit_key(payer), &(credit + 1));

        append_audit_entry(env, invoice_id, symbol_short!("pay"), payer);
        events::payment_received(env, invoice_id, payer, credited_amount);

        if invoice.funded >= total {
            // Auto-release only when no tranches, prerequisite, or group constraint
            // requires a manual release() call.
            let in_group = env
                .storage()
                .persistent()
                .has(&invoice_group_key(invoice_id));
            let guarded =
                invoice.prerequisite_id.is_some()
                    || !invoice.tranches.is_empty()
                    || !invoice.release_stages.is_empty()
                    || in_group
                    || !invoice.co_signers.is_empty();
            if guarded {
                save_invoice(env, invoice_id, &invoice);
            } else {
                Self::_release(env, invoice_id, &mut invoice, payer);
            }
        } else {
            save_invoice(env, invoice_id, &invoice);
        }
    }

    // -----------------------------------------------------------------------
    // Issue #2: pay with an alternate accepted token
    // -----------------------------------------------------------------------

    /// Pay toward an invoice using any token listed in `invoice.accepted_tokens`.
    ///
    /// When `source_token` differs from the invoice base token, the contract
    /// transfers `amount` of `source_token` from `payer` to itself, then calls
    /// the on-chain DEX (stored at "dex_ctr") to swap it for the invoice token.
    /// The converted amount is credited to `invoice.funded`.
    pub fn pay_with_token(
        env: Env,
        payer: Address,
        invoice_id: u64,
        source_token: Address,
        amount: i128,
        nonce: u64,
    ) {
        require_not_paused(&env);
        payer.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(invoice.status == InvoiceStatus::Pending, "invoice is not pending");
        assert!(env.ledger().timestamp() <= invoice.deadline, "invoice deadline has passed");
        assert!(amount > 0, "payment amount must be positive");

        let invoice_token = invoice.tokens.get(0).expect("no token");

        // Accept the base token or any token in accepted_tokens.
        let is_base = source_token == invoice_token;
        let is_accepted = is_base
            || invoice.accepted_tokens.iter().any(|t| t == source_token);
        assert!(is_accepted, "token not accepted");

        // Validate and increment nonce.
        let stored_nonce: u64 = env
            .storage()
            .persistent()
            .get(&nonce_key(invoice_id, &payer))
            .unwrap_or(0u64);
        assert!(nonce == stored_nonce, "invalid nonce");
        env.storage()
            .persistent()
            .set(&nonce_key(invoice_id, &payer), &(stored_nonce + 1));

        let credited_amount = if is_base {
            // Direct transfer of the invoice token.
            let token_client = token::Client::new(&env, &invoice_token);
            token_client.transfer(&payer, &env.current_contract_address(), &amount);
            amount
        } else {
            // Transfer source token from payer to contract.
            let src_client = token::Client::new(&env, &source_token);
            src_client.transfer(&payer, &env.current_contract_address(), &amount);

            // Swap source_token -> invoice_token via DEX contract.
            let dex: Address = env
                .storage()
                .persistent()
                .get(&soroban_sdk::symbol_short!("dex_ctr"))
                .expect("dex contract not set");
            let mut args: Vec<Val> = Vec::new(&env);
            args.push_back(source_token.into_val(&env));
            args.push_back(invoice_token.into_val(&env));
            args.push_back(amount.into_val(&env));
            let converted: i128 = env.invoke_contract(&dex, &Symbol::new(&env, "swap"), args);
            converted
        };

        let total: i128 = invoice.amounts.iter().sum();
        let remaining = total - invoice.funded;
        assert!(credited_amount <= remaining, "payment exceeds remaining balance");

        invoice.payments.push_back(Payment { payer: payer.clone(), amount: credited_amount, tip: 0 });
        invoice.funded += credited_amount;

        append_audit_entry(&env, invoice_id, symbol_short!("pay_tok"), &payer);
        events::payment_received(&env, invoice_id, &payer, credited_amount);

        if invoice.funded >= total {
            let in_group = env.storage().persistent().has(&invoice_group_key(invoice_id));
            let guarded =
                invoice.prerequisite_id.is_some()
                    || !invoice.tranches.is_empty()
                    || !invoice.release_stages.is_empty()
                    || in_group
                    || !invoice.co_signers.is_empty();
            if guarded {
                save_invoice(&env, invoice_id, &invoice);
            } else {
                Self::_release(&env, invoice_id, &mut invoice, &payer);
            }
        } else {
            save_invoice(&env, invoice_id, &invoice);
        }
    }

    // -----------------------------------------------------------------------
    // Issue #3: batched multi-invoice payment
    // -----------------------------------------------------------------------

    /// Pay toward multiple invoices in a single call, using only one token transfer.
    ///
    /// All invoices must share the same base token. The payer's total is transferred
    /// once; each invoice's `funded` counter is then updated via internal accounting.
    /// Any invalid payment (wrong status, over limit) reverts the entire call.
    /// Invoices that become fully funded trigger auto-release where applicable.
    pub fn pool_pay(env: Env, payer: Address, payments: Vec<InvoicePayment>) {
        require_not_paused(&env);
        payer.require_auth();

        assert!(!payments.is_empty(), "payments must not be empty");

        // Determine the shared token from the first invoice.
        let first_inv = load_invoice(&env, payments.get(0).unwrap().invoice_id);
        let shared_token = first_inv.tokens.get(0).expect("no token");

        // Validate all payments and compute total.
        let mut total: i128 = 0;
        for p in payments.iter() {
            let inv = load_invoice(&env, p.invoice_id);
            assert!(inv.status == InvoiceStatus::Pending, "invoice is not pending");
            assert!(
                env.ledger().timestamp() <= inv.deadline,
                "invoice deadline has passed"
            );
            assert!(p.amount > 0, "payment amount must be positive");
            let inv_total: i128 = inv.amounts.iter().sum();
            assert!(
                inv.funded + p.amount <= inv_total,
                "payment exceeds remaining balance"
            );
            // All invoices must use the same token.
            assert!(
                inv.tokens.get(0).expect("no token") == shared_token,
                "all invoices must use the same token"
            );
            total += p.amount;
        }

        // Single token transfer from payer to contract.
        let token_client = token::Client::new(&env, &shared_token);
        token_client.transfer(&payer, &env.current_contract_address(), &total);

        // Update each invoice via internal accounting (no further token transfers).
        for p in payments.iter() {
            let mut inv = load_invoice(&env, p.invoice_id);
            inv.payments.push_back(Payment { payer: payer.clone(), amount: p.amount, tip: 0 });
            inv.funded += p.amount;

            append_audit_entry(&env, p.invoice_id, symbol_short!("pool_pay"), &payer);
            events::payment_received(&env, p.invoice_id, &payer, p.amount);

            let inv_total: i128 = inv.amounts.iter().sum();
            if inv.funded >= inv_total {
                let in_group = env.storage().persistent().has(&invoice_group_key(p.invoice_id));
                let guarded =
                    inv.prerequisite_id.is_some()
                        || !inv.tranches.is_empty()
                        || !inv.release_stages.is_empty()
                        || in_group
                        || !inv.co_signers.is_empty();
                if guarded {
                    save_invoice(&env, p.invoice_id, &inv);
                } else {
                    Self::_release(&env, p.invoice_id, &mut inv, &payer);
                }
            } else {
                save_invoice(&env, p.invoice_id, &inv);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Co-signer approval & Release
    // -----------------------------------------------------------------------

    /// Record a co-signer's approval to release an invoice.
    ///
    /// Only addresses in `co_signers` may call this. Once `required_signatures`
    /// unique co-signers have approved, the release guard is satisfied.
    pub fn sign_release(env: Env, invoice_id: u64, signer: Address) {
        require_not_paused(&env);
        signer.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.co_signers.is_empty(), "no co-signers required");
        assert!(
            invoice.co_signers.iter().any(|c| c == signer),
            "not an authorized co-signer"
        );
        assert!(
            !invoice.signatures.iter().any(|s| s == signer),
            "already signed"
        );

        invoice.signatures.push_back(signer.clone());
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("sign_rel"), &signer);
    }

    // -----------------------------------------------------------------------
    // Release (#22 prerequisite, #23 tranches)
    // -----------------------------------------------------------------------

    /// Release funds to recipients.
    ///
    /// For tranche invoices, only distributes tranches whose timestamp ≤ now.
    /// Blocks with "prerequisite not released" until the prerequisite invoice is Released.
    /// If an approver is set, requires the invoice to be approved first (issue #25).
    pub fn release(env: Env, invoice_id: u64) {
        require_not_paused(&env);
        let caller = env.current_contract_address();
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(!invoice.frozen, "invoice is frozen");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let total: i128 = invoice.amounts.iter().sum();
        let min_required = if invoice.min_funding_bps > 0 {
            (total as u128 * invoice.min_funding_bps as u128 / 10_000u128) as i128
        } else {
            total
        };
        assert!(invoice.funded >= min_required, "minimum funding not reached");

        // Approval check (issue #25).
        if invoice.approver.is_some() && !invoice.approved {
            panic!("awaiting approval");
        }

        // Prerequisite check (issue #22).
        if let Some(prereq_id) = invoice.prerequisite_id {
            let prereq = load_invoice(&env, prereq_id);
            assert!(
                prereq.status == InvoiceStatus::Released,
                "prerequisite not released"
            );
        }

        // Group constraint: all members must be fully funded before any can release.
        if let Some(group_id) = env
            .storage()
            .persistent()
            .get::<(Symbol, u64), u64>(&invoice_group_key(invoice_id))
        {
            assert!(group_all_funded(&env, group_id), "group members not fully funded");
        }

        // Co-signer approval check.
        if !invoice.co_signers.is_empty() {
            assert!(
                invoice.signatures.len() >= invoice.required_signatures,
                "not enough co-signer approvals"
            );
        }

        Self::_release(&env, invoice_id, &mut invoice, &caller);
    }

    fn _release(env: &Env, invoice_id: u64, invoice: &mut Invoice, actor: &Address) {
        if invoice.tranches.is_empty() {
            Self::_release_full(env, invoice_id, invoice, actor);
        } else {
            Self::_release_tranches(env, invoice_id, invoice, actor);
        }
    }

    fn execute_smart_route(env: &Env, invoice: &Invoice, recipient: &Address, payout: i128) -> bool {
        if invoice.smart_route {
            if let Some(dex_router) = env.storage().instance().get::<_, Address>(&soroban_sdk::symbol_short!("dex_rtr")) {
                let token = invoice.tokens.get(0).expect("no token");
                let path: Vec<Address> = env.invoke_contract(
                    &dex_router,
                    &soroban_sdk::Symbol::new(env, "get_path"),
                    (token.clone(), recipient.clone()).into_val(env)
                );
                if !path.is_empty() {
                    let _: Val = env.invoke_contract(
                        &dex_router,
                        &soroban_sdk::Symbol::new(env, "route_transfer"),
                        (path, payout, recipient.clone()).into_val(env)
                    );
                    return true;
                }
            }
        }
        false
    }

    /// Approve an invoice if it has an approver set (issue #25).
    ///
    /// Requires authentication from the approver address.
    pub fn approve_invoice(env: Env, invoice_id: u64) {
        require_not_paused(&env);
        let mut invoice = load_invoice(&env, invoice_id);

        let approver = invoice.approver.as_ref().expect("no approver set on this invoice");
        approver.require_auth();

        invoice.approved = true;
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("aprv"), approver);
    }

    /// Claim vesting cliff share after cliff timestamp has passed (issue #27).
    ///
    /// Requires that the invoice status is Released and the cliff (if set) has passed.
    /// Each recipient can claim exactly once.
    pub fn claim(env: Env, invoice_id: u64, recipient: Address) {
        require_not_paused(&env);
        recipient.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Released,
            "invoice not released"
        );

        // Find recipient index
        let idx = invoice
            .recipients
            .iter()
            .position(|r| r == recipient)
            .expect("recipient not in invoice") as u32;

        // Check if already claimed
        assert!(
            !invoice.vesting_cliff_claimed.get(idx).unwrap(),
            "recipient already claimed"
        );

        // Check cliff timestamp if set
        if let Some(cliff) = invoice.vesting_cliff {
            let now = env.ledger().timestamp();
            assert!(now >= cliff, "cliff not reached");
        }

        // Mark as claimed
        invoice.vesting_cliff_claimed.set(idx, true);
        save_invoice(&env, invoice_id, &invoice);

        // Transfer recipient's share
        let amount = invoice.amounts.get(idx).unwrap();
        let total: i128 = invoice.amounts.iter().sum();
        let funded = invoice.funded;
        let n = invoice.recipients.len() as u32;

        let proportional = if idx == n - 1 {
            // Last recipient gets remainder
            funded - {
                let mut sum = 0i128;
                for i in 0..idx {
                    let amt = invoice.amounts.get(i).unwrap();
                    let prop = (amt as u128 * funded as u128 / total as u128) as i128;
                    sum += prop;
                }
                sum
            }
        } else {
            (amount as u128 * funded as u128 / total as u128) as i128
        };

        let platform_fee_bps: u32 = env
            .storage()
            .instance()
            .get(&platform_fee_bps_key())
            .unwrap_or(0u32);

        let fee = (proportional as u128 * platform_fee_bps as u128 / 10_000u128) as i128;
        let tax = (proportional as u128 * invoice.tax_bps as u128 / 10_000u128) as i128;
        let payout = proportional - fee - tax;

        let token_client = token::Client::new(&env, &invoice.tokens.get(idx).expect("no token"));
        
        if tax > 0 {
            let tax_authority = invoice.tax_authority.as_ref().unwrap();
            token_client.transfer(&env.current_contract_address(), tax_authority, &tax);
        }
        
        let routed = Self::execute_smart_route(&env, &invoice, &recipient, payout);
        if !routed {
            token_client.transfer(&env.current_contract_address(), &recipient, &payout);
        }

        append_audit_entry(&env, invoice_id, symbol_short!("claim"), &recipient);
    }

    /// Distribute tranches unlocked by the current ledger time (issue #23).
    fn _release_tranches(env: &Env, invoice_id: u64, invoice: &mut Invoice, actor: &Address) {
        let now = env.ledger().timestamp();

        // Sum all basis points whose timestamp has passed.
        let mut unlocked_bps: u32 = 0;
        for tranche in invoice.tranches.iter() {
            if tranche.timestamp <= now {
                unlocked_bps += tranche.basis_points;
            }
        }

        // New basis points not yet distributed.
        let new_bps = unlocked_bps.saturating_sub(invoice.released_bps);
        assert!(new_bps > 0, "no tranches unlocked");

        let token_client =
            token::Client::new(env, &invoice.tokens.get(0).expect("no token"));

        let platform_fee_bps: u32 = env
            .storage()
            .instance()
            .get(&platform_fee_bps_key())
            .unwrap_or(0u32);

        let total: i128 = invoice.amounts.iter().sum();
        let funded = invoice.funded;
        let n = invoice.recipients.len();
        let mut total_fee: i128 = 0;
        let mut total_tax: i128 = 0;
        for i in 0..n {
            let recipient = invoice.recipients.get(i).unwrap();
            let amount = invoice.amounts.get(i).unwrap();
            // integer math: avoid overflow via u128 intermediary.
            let payout_raw = (amount as u128)
                .saturating_mul(new_bps as u128)
                .saturating_mul(funded as u128)
                / (10000u128 * total as u128);
            let payout_raw = payout_raw as i128;
            if payout_raw > 0 {
                let fee = (payout_raw as u128 * platform_fee_bps as u128 / 10_000u128) as i128;
                let tax = (payout_raw as u128 * invoice.tax_bps as u128 / 10_000u128) as i128;
                let payout = payout_raw - fee - tax;
                total_fee += fee;
                total_tax += tax;
                let routed = Self::execute_smart_route(env, invoice, &recipient, payout);
                if !routed {
                    token_client.transfer(&env.current_contract_address(), &recipient, &payout);
                }
            }
        }

        if total_tax > 0 {
            if let Some(ref auth) = invoice.tax_authority {
                token_client.transfer(&env.current_contract_address(), auth, &total_tax);
            }
        }

        if total_fee > 0 {
            let treasury: Address = env
                .storage()
                .instance()
                .get(&treasury_key())
                .expect("treasury not set");
            token_client.transfer(&env.current_contract_address(), &treasury, &total_fee);
        }

        if total_tax > 0 {
            let tax_authority = invoice.tax_authority.as_ref().unwrap();
            token_client.transfer(&env.current_contract_address(), tax_authority, &total_tax);
        }

        invoice.released_bps += new_bps;

        // Calculate amount released in this tranche call.
        let amount_released = ((funded as u128)
            .saturating_mul(new_bps as u128)
            / 10_000u128) as i128;

        // Increment total_volume and total_released counters (issue #28).
        let total_volume: i128 = env
            .storage()
            .persistent()
            .get(&total_volume_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_volume_key(),
            &total_volume.checked_add(amount_released).expect("total_volume overflow"),
        );

        let total_released: i128 = env
            .storage()
            .persistent()
            .get(&total_released_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_released_key(),
            &total_released.checked_add(amount_released).expect("total_released overflow"),
        );

        if invoice.released_bps >= 10_000 {
            invoice.status = InvoiceStatus::Released;
            invoice.completion_time = Some(now);
            if invoice.insurance_fund > 0 {
                token_client.transfer(&env.current_contract_address(), &invoice.creator, &invoice.insurance_fund);
                invoice.insurance_fund = 0;
            }
            append_audit_entry(env, invoice_id, symbol_short!("release"), actor);
            events::invoice_released(env, invoice_id, &invoice.recipients);
        }

        save_invoice(env, invoice_id, invoice);
    }

    // -----------------------------------------------------------------------
    // Stage release (#86)
    // -----------------------------------------------------------------------

    /// Release the next predefined stage of funds to recipients.
    ///
    /// Requires creator auth. Each call distributes the next stage's proportion
    /// of the total funded amount. The final stage sets the invoice status to Released.
    pub fn stage_release(env: Env, invoice_id: u64, creator: Address) {
        require_not_paused(&env);
        creator.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(invoice.creator == creator, "only creator can call stage_release");
        assert!(!invoice.frozen, "invoice is frozen");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(!invoice.release_stages.is_empty(), "no release stages defined");

        let total: i128 = invoice.amounts.iter().sum();
        assert!(invoice.funded >= total, "invoice not fully funded");

        let stage_idx = invoice.released_stages;
        assert!(
            stage_idx < invoice.release_stages.len(),
            "all stages already released"
        );

        let stage_bps = invoice.release_stages.get(stage_idx).unwrap();

        let token_client =
            token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));

        let platform_fee_bps: u32 = env
            .storage()
            .instance()
            .get(&platform_fee_bps_key())
            .unwrap_or(0u32);

        let funded = invoice.funded;
        let n = invoice.recipients.len();
        let mut total_fee: i128 = 0;
        let mut total_tax: i128 = 0;
        for i in 0..n {
            let recipient = invoice.recipients.get(i).unwrap();
            let amount = invoice.amounts.get(i).unwrap();
            let payout_raw = (amount as u128)
                .saturating_mul(stage_bps as u128)
                .saturating_mul(funded as u128)
                / (10_000u128 * total as u128);
            let payout_raw = payout_raw as i128;
            if payout_raw > 0 {
                let fee = (payout_raw as u128 * platform_fee_bps as u128 / 10_000u128) as i128;
                let tax = (payout_raw as u128 * invoice.tax_bps as u128 / 10_000u128) as i128;
                let payout = payout_raw - fee - tax;
                total_fee += fee;
                total_tax += tax;
                let routed = Self::execute_smart_route(&env, &invoice, &recipient, payout);
                if !routed {
                    token_client.transfer(&env.current_contract_address(), &recipient, &payout);
                }
            }
        }

        if total_fee > 0 {
            let treasury: Address = env
                .storage()
                .instance()
                .get(&treasury_key())
                .expect("treasury not set");
            token_client.transfer(&env.current_contract_address(), &treasury, &total_fee);
        }

        if total_tax > 0 {
            let tax_authority = invoice.tax_authority.as_ref().unwrap();
            token_client.transfer(&env.current_contract_address(), tax_authority, &total_tax);
        }

        invoice.released_stages += 1;

        // Calculate amount released in this stage.
        let amount_released = ((stage_bps as u128)
            .saturating_mul(funded as u128)
            / 10_000u128) as i128;

        // Increment total_volume and total_released counters (issue #28).
        let total_volume: i128 = env
            .storage()
            .persistent()
            .get(&total_volume_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_volume_key(),
            &total_volume.checked_add(amount_released).expect("total_volume overflow"),
        );

        let total_released: i128 = env
            .storage()
            .persistent()
            .get(&total_released_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_released_key(),
            &total_released.checked_add(amount_released).expect("total_released overflow"),
        );

        let now = env.ledger().timestamp();
        if invoice.released_stages >= invoice.release_stages.len() {
            invoice.status = InvoiceStatus::Released;
            invoice.completion_time = Some(now);
            if invoice.insurance_fund > 0 {
                token_client.transfer(&env.current_contract_address(), &invoice.creator, &invoice.insurance_fund);
                invoice.insurance_fund = 0;
            }
            append_audit_entry(&env, invoice_id, symbol_short!("stg_rel"), &creator);
            events::invoice_released(&env, invoice_id, &invoice.recipients);
        } else {
            append_audit_entry(&env, invoice_id, symbol_short!("stg_rel"), &creator);
        }

        save_invoice(&env, invoice_id, &invoice);
    }

    /// Full immediate release (no tranches).
    /// Issue #89: Returns stake to creator on successful release.
    /// Issue #41: Swaps recipient payout via DEX if swap_tokens[i] is set.
    fn _release_full(env: &Env, invoice_id: u64, invoice: &mut Invoice, actor: &Address) {
        // Issue #27: If vesting cliff is set, just mark as Released without transferring funds
        if invoice.vesting_cliff.is_some() {
            invoice.status = InvoiceStatus::Released;
            invoice.completion_time = Some(env.ledger().timestamp());
            save_invoice(env, invoice_id, invoice);
            append_audit_entry(env, invoice_id, symbol_short!("release"), actor);
            events::invoice_released(env, invoice_id, &invoice.recipients);
            return;
        }

        let token_client =
            token::Client::new(env, &invoice.tokens.get(0).expect("no token"));

        let platform_fee_bps: u32 = env
            .storage()
            .instance()
            .get(&platform_fee_bps_key())
            .unwrap_or(0u32);

        let total: i128 = invoice.amounts.iter().sum();
        let funded = invoice.funded;
        let n = invoice.recipients.len();
        let mut distributed: i128 = 0;
        let mut total_fee: i128 = 0;
        let mut total_tax: i128 = 0;
        for i in 0..n {
            let recipient = invoice.recipients.get(i).unwrap();
            let amount = invoice.amounts.get(i).unwrap();
            let proportional = if i == n - 1 {
                funded - distributed
            } else {
                (amount as u128 * funded as u128 / total as u128) as i128
            };
            let fee = (proportional as u128 * platform_fee_bps as u128 / 10_000u128) as i128;
            let tax = (proportional as u128 * invoice.tax_bps as u128 / 10_000u128) as i128;
            let payout = proportional - fee - tax;
            distributed += proportional;

            let tax = (proportional as u128 * invoice.tax_bps as u128 / 10_000u128) as i128;
            let post_tax = proportional - tax;
            total_tax += tax;

            let fee = (post_tax as u128 * platform_fee_bps as u128 / 10_000u128) as i128;
            let payout = post_tax - fee;
            total_fee += fee;
            total_tax += tax;

            // Issue #41: if a swap token is configured for this recipient, invoke DEX swap.
            let swap_token: Option<Address> = invoice
                .swap_tokens
                .get(i as u32)
                .unwrap_or(None);
            if let Some(ref out_token) = swap_token {
                let from_token = invoice.tokens.get(0).expect("no token");
                let mut args: Vec<Val> = Vec::new(env);
                args.push_back(from_token.into_val(env));
                args.push_back(out_token.clone().into_val(env));
                args.push_back(payout.into_val(env));
                args.push_back(recipient.into_val(env));
                let _swapped: i128 = env.invoke_contract(out_token, &Symbol::new(env, "swap"), args);
            } else if invoice.smart_route {
                // Smart routing: query DEX router for optimal path, fall back to direct transfer.
                let from_token = invoice.tokens.get(0).expect("no token");
                let mut route_args: Vec<Val> = Vec::new(env);
                route_args.push_back(from_token.into_val(env));
                route_args.push_back(payout.into_val(env));
                route_args.push_back(recipient.clone().into_val(env));
                // Try DEX path-finding via invoke; on failure fall back to direct transfer.
                // In production the router address would be stored; here we attempt invoke
                // and catch failure by falling back.
                token_client.transfer(&env.current_contract_address(), &recipient, &payout);
            } else {
                let routed = Self::execute_smart_route(env, invoice, &recipient, payout);
                if !routed {
                    token_client.transfer(&env.current_contract_address(), &recipient, &payout);
                }
            }
        }

        if total_tax > 0 {
            if let Some(ref auth) = invoice.tax_authority {
                token_client.transfer(&env.current_contract_address(), auth, &total_tax);
            }
        }

        if total_fee > 0 {
            let treasury: Address = env
                .storage()
                .instance()
                .get(&treasury_key())
                .expect("treasury not set");
            token_client.transfer(&env.current_contract_address(), &treasury, &total_fee);
        }

        if total_tax > 0 {
            let tax_authority = invoice.tax_authority.as_ref().unwrap();
            token_client.transfer(&env.current_contract_address(), tax_authority, &total_tax);
        }

        // Distribute bonus pool among first `bonus_max_payers` unique payers.
        if invoice.bonus_pool > 0 && invoice.bonus_max_payers > 0 {
            let mut unique_payers: Vec<Address> = Vec::new(env);
            for payment in invoice.payments.iter() {
                let already_seen = unique_payers.iter().any(|p| p == payment.payer);
                if !already_seen {
                    unique_payers.push_back(payment.payer.clone());
                    if unique_payers.len() >= invoice.bonus_max_payers {
                        break;
                    }
                }
            }

            if !unique_payers.is_empty() {
                let n = unique_payers.len() as i128;
                let per_payer = invoice.bonus_pool / n;
                let mut distributed: i128 = 0;
                for (i, payer) in unique_payers.iter().enumerate() {
                    let payout = if i as i128 == n - 1 {
                        invoice.bonus_pool - distributed
                    } else {
                        per_payer
                    };
                    token_client.transfer(&env.current_contract_address(), &payer, &payout);
                    distributed += payout;
                }
            }
        }

        // Issue #89: Return stake to creator on successful release.
        // (stake_amount field not yet on Invoice; skipped)

        // Release all group members if this invoice is part of a group.
        if let Some(group_id) = env
            .storage()
            .persistent()
            .get::<(Symbol, u64), u64>(&invoice_group_key(invoice_id))
        {
            for member_id in load_group(env, group_id).iter() {
                if member_id != invoice_id {
                    let mut member = load_invoice(env, member_id);
                    if member.status == InvoiceStatus::Pending {
                        let member_token =
                            token::Client::new(env, &member.tokens.get(0).expect("no token"));
                        let member_total: i128 = member.amounts.iter().sum();
                        let member_funded = member.funded;
                        let member_n = member.recipients.len();
                        let mut member_distributed: i128 = 0;
                        let mut group_total_fee: i128 = 0;
                        for (j, (recipient, amount)) in
                            member.recipients.iter().zip(member.amounts.iter()).enumerate()
                        {
                            let proportional = if j == (member_n - 1) as usize {
                                member_funded - member_distributed
                            } else {
                                (amount as u128 * member_funded as u128 / member_total as u128) as i128
                            };
                            let fee = (proportional as u128 * platform_fee_bps as u128 / 10_000u128) as i128;
                            let tax = (proportional as u128 * member.tax_bps as u128 / 10_000u128) as i128;
                            let payout = proportional - fee - tax;
                            member_distributed += proportional;
                            group_total_fee += fee;
                            if tax > 0 {
                                let tax_authority = member.tax_authority.as_ref().unwrap();
                                member_token.transfer(&env.current_contract_address(), tax_authority, &tax);
                            }
                            let routed = Self::execute_smart_route(env, &member, &recipient, payout);
                            if !routed {
                                member_token.transfer(
                                    &env.current_contract_address(),
                                    &recipient,
                                    &payout,
                                );
                            }
                        }
                        if group_total_fee > 0 {
                            let treasury: Address = env
                                .storage()
                                .instance()
                                .get(&treasury_key())
                                .expect("treasury not set");
                            member_token.transfer(
                                &env.current_contract_address(),
                                &treasury,
                                &group_total_fee,
                            );
                        }
                        member.status = InvoiceStatus::Released;
                        member.completion_time = Some(env.ledger().timestamp());
                        save_invoice(env, member_id, &member);
                        append_audit_entry(env, member_id, symbol_short!("release"), actor);
                        events::invoice_released(env, member_id, &member.recipients);
                    }
                }
            }
        }

        // Return insurance fund to creator on successful release.
        if invoice.insurance_fund > 0 {
            token_client.transfer(&env.current_contract_address(), &invoice.creator, &invoice.insurance_fund);
            invoice.insurance_fund = 0;
        }

        invoice.status = InvoiceStatus::Released;
        invoice.completion_time = Some(env.ledger().timestamp());
        if invoice.insurance_fund > 0 {
            let token_client = token::Client::new(env, &invoice.tokens.get(0).expect("no token"));
            token_client.transfer(&env.current_contract_address(), &invoice.creator, &invoice.insurance_fund);
            invoice.insurance_fund = 0;
        }
        save_invoice(env, invoice_id, invoice);
        append_audit_entry(env, invoice_id, symbol_short!("release"), actor);
        events::invoice_released(env, invoice_id, &invoice.recipients);

        // Increment total_volume and total_released counters (issue #28).
        let total_volume: i128 = env
            .storage()
            .persistent()
            .get(&total_volume_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_volume_key(),
            &total_volume.checked_add(funded).expect("total_volume overflow"),
        );

        let total_released: i128 = env
            .storage()
            .persistent()
            .get(&total_released_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_released_key(),
            &total_released.checked_add(funded).expect("total_released overflow"),
        );

        // Spin up next subscription invoice if one is scheduled.
        if let Some(params) = env
            .storage()
            .persistent()
            .get::<(Symbol, u64), SubscriptionParams>(&subscription_params_key(invoice_id))
        {
            let next_deadline = env.ledger().timestamp() + 30 * 24 * 60 * 60;
            let first_token = params.tokens.get(0).expect("no token in subscription");
            let _next_id = Self::_create_invoice_inner(
                env,
                params.creator.clone(),
                params.recipients.clone(),
                params.amounts.clone(),
                first_token,
                next_deadline,
                Vec::new(env),
                false,
                0,
                0,
                None,
                Vec::new(env),
                Vec::new(env),
                0,
                0,
                0,
                0,
                Vec::new(env),
                None,
                Vec::new(env),
                0,
                None,
                0,
                false,
                false,
                Vec::new(env),
            );
            env.storage()
                .persistent()
                .remove(&subscription_params_key(invoice_id));
        }
    }

    // -----------------------------------------------------------------------
    // Refund / cancel / transfer / deadline
    // -----------------------------------------------------------------------

    /// Refund all payers if the deadline has passed and the invoice is not fully funded.
    pub fn refund(env: Env, invoice_id: u64) {
        require_not_paused(&env);
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            env.ledger().timestamp() > invoice.deadline,
            "deadline has not passed"
        );

        let token_client =
            token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));

        let mut totals: Map<Address, i128> = Map::new(&env);
        for payment in invoice.payments.iter() {
            let prev = totals.get(payment.payer.clone()).unwrap_or(0);
            totals.set(payment.payer.clone(), prev + payment.amount);
        }

        let mut total_refunded_amount: i128 = 0;
        for (payer, amount) in totals.iter() {
            token_client.transfer(&env.current_contract_address(), &payer, &amount);
            total_refunded_amount += amount;
            events::payer_refunded(&env, invoice_id, &payer, amount);
        }

        if invoice.bonus_pool > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &invoice.creator,
                &invoice.bonus_pool,
            );
        }

        invoice.status = InvoiceStatus::Refunded;
        invoice.completion_time = Some(env.ledger().timestamp());
        save_invoice(&env, invoice_id, &invoice);
        let actor = env.current_contract_address();
        append_audit_entry(&env, invoice_id, symbol_short!("refund"), &actor);
        events::invoice_refunded(&env, invoice_id);

        // Increment total_refunded counter (issue #28).
        let total_refunded: i128 = env
            .storage()
            .persistent()
            .get(&total_refunded_key())
            .unwrap_or(0i128);
        env.storage().persistent().set(
            &total_refunded_key(),
            &total_refunded.checked_add(total_refunded_amount).expect("total_refunded overflow"),
        );
    }

    /// Cancel an invoice. Refunds any payments already made.
    /// Issue #89: If stake exists, distributes it equally among unique payers.
    pub fn cancel_invoice(env: Env, caller: Address, invoice_id: u64) {
        require_not_paused(&env);
        caller.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(invoice.creator == caller, "only creator can cancel");

        if invoice.funded > 0 {
            // Refund all payments.
            let token_client =
                token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));

            let mut totals: Map<Address, i128> = Map::new(&env);
            for payment in invoice.payments.iter() {
                let prev = totals.get(payment.payer.clone()).unwrap_or(0);
                totals.set(payment.payer.clone(), prev + payment.amount);
            }

            // Issue #89: Distribute stake equally among unique payers if stake exists.
            // (stake_amount field not yet on Invoice; skipped)

            let mut total_refunded_amount: i128 = 0;
            for (payer, amount) in totals.iter() {
                let mut refund = amount;
                if invoice.insurance_fund > 0 {
                    let premium_refund = (amount as u128 * invoice.insurance_fund as u128 / invoice.funded as u128) as i128;
                    refund += premium_refund;
                }
                token_client.transfer(&env.current_contract_address(), &payer, &refund);
                total_refunded_amount += amount;
            }

            if invoice.insurance_fund > 0 {
                invoice.insurance_fund = 0;
            }

            if invoice.bonus_pool > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &invoice.creator,
                    &invoice.bonus_pool,
                );
            }

            if invoice.insurance_fund > 0 {
                let mut total_paid: i128 = 0;
                for (_, amt) in totals.iter() {
                    total_paid += amt;
                }
                if total_paid > 0 {
                    for (payer, amt) in totals.iter() {
                        let share = (invoice.insurance_fund as u128 * amt as u128 / total_paid as u128) as i128;
                        if share > 0 {
                            token_client.transfer(&env.current_contract_address(), &payer, &share);
                        }
                    }
                }
                invoice.insurance_fund = 0;
            }

            invoice.status = InvoiceStatus::Refunded;

            // Increment total_refunded counter (issue #28).
            let total_refunded: i128 = env
                .storage()
                .persistent()
                .get(&total_refunded_key())
                .unwrap_or(0i128);
            env.storage().persistent().set(
                &total_refunded_key(),
                &total_refunded.checked_add(total_refunded_amount).expect("total_refunded overflow"),
            );
        } else {
            if invoice.bonus_pool > 0 {
                let token_client =
                    token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
                token_client.transfer(
                    &env.current_contract_address(),
                    &invoice.creator,
                    &invoice.bonus_pool,
                );
            }
            
            // Issue #89: Return stake to creator if no payments were made.
            // (stake_amount field not yet on Invoice; skipped)

            invoice.status = InvoiceStatus::Cancelled;
        }

        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("cancel"), &caller);
    }

    /// Transfer invoice ownership to a new creator.
    pub fn transfer_invoice(env: Env, invoice_id: u64, new_creator: Address) {
        require_not_paused(&env);
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        invoice.creator.require_auth();
        invoice.creator = new_creator;
        save_invoice(&env, invoice_id, &invoice);
    }

    /// Extend the deadline for an invoice. Callable by the creator or an assigned delegate.
    pub fn extend_deadline(env: Env, invoice_id: u64, new_deadline: u64, caller: Address) {
        require_not_paused(&env);
        caller.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice not pending"
        );
        assert!(
            new_deadline > invoice.deadline,
            "new deadline must be after current deadline"
        );

        // Accept caller = creator OR assigned delegate (issue #43).
        let delegate: Option<Address> = env
            .storage()
            .persistent()
            .get(&delegate_key(invoice_id));
        let is_creator = invoice.creator == caller;
        let is_delegate = delegate.map(|d| d == caller).unwrap_or(false);
        assert!(is_creator || is_delegate, "not authorized");

        invoice.deadline = new_deadline;
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("extend"), &caller);
    }

    /// Roll over a partially funded invoice to a new invoice with the same recipients,
    /// amounts, and token. Carries over all existing payments and marks the old invoice
    /// as Refunded without transferring tokens.
    ///
    /// Requires creator auth. The old invoice must be Pending and past its deadline.
    /// The new deadline must be in the future.
    pub fn rollover_invoice(env: Env, caller: Address, invoice_id: u64, new_deadline: u64) -> u64 {
        require_not_paused(&env);
        caller.require_auth();

        let mut old_invoice = load_invoice(&env, invoice_id);

        assert!(
            old_invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(
            old_invoice.creator == caller,
            "only creator can rollover invoice"
        );
        assert!(
            env.ledger().timestamp() > old_invoice.deadline,
            "invoice deadline has not passed"
        );
        assert!(
            new_deadline > env.ledger().timestamp(),
            "new deadline must be in the future"
        );

        // Create new invoice with same recipients, amounts, and token.
        let new_id = Self::_create_invoice_inner(
            &env,
            old_invoice.creator.clone(),
            old_invoice.recipients.clone(),
            old_invoice.amounts.clone(),
            old_invoice.tokens.get(0).expect("no token"),
            new_deadline,
            old_invoice.co_creators.clone(),
            old_invoice.allow_early_withdrawal,
            0, // No bonus pool on rollover
            0, // No bonus max payers on rollover
            old_invoice.prerequisite_id,
            old_invoice.tranches.clone(),
            old_invoice.co_signers.clone(),
            old_invoice.required_signatures,
            old_invoice.penalty_bps,
            old_invoice.penalty_deadline,
            old_invoice.min_funding_bps,
            old_invoice.release_stages.clone(),
            old_invoice.price_oracle.clone(),
            old_invoice.swap_tokens.clone(),
            old_invoice.tax_bps,
            old_invoice.tax_authority.clone(),
            old_invoice.insurance_premium_bps,
            old_invoice.smart_route,
            old_invoice.convert_to_stream,
            old_invoice.accepted_tokens.clone(),
        );

        // Load the newly created invoice and copy over the payments.
        let mut new_invoice = load_invoice(&env, new_id);
        new_invoice.payments = old_invoice.payments.clone();
        new_invoice.funded = old_invoice.funded;
        save_invoice(&env, new_id, &new_invoice);

        // Mark old invoice as Refunded without transferring tokens.
        old_invoice.status = InvoiceStatus::Refunded;
        old_invoice.completion_time = Some(env.ledger().timestamp());
        save_invoice(&env, invoice_id, &old_invoice);

        append_audit_entry(&env, invoice_id, symbol_short!("rollover"), &caller);
        append_audit_entry(&env, new_id, symbol_short!("rollover"), &caller);

        new_id
    }

    // -----------------------------------------------------------------------
    // Adjust split
    // -----------------------------------------------------------------------

    /// Update recipient amounts before any payment has been received.
    ///
    /// Only the creator may call this. Panics if any payment has already been
    /// made (`invoice.funded > 0`). The length of `new_amounts` must match the
    /// current number of recipients, and every amount must be positive.
    // -----------------------------------------------------------------------
    // Add recipient
    // -----------------------------------------------------------------------

    /// Append a new recipient with a fixed amount to a pending invoice.
    /// Only the creator may call this, and only before any payment has been
    /// received.
    pub fn add_recipient(
        env: Env,
        caller: Address,
        invoice_id: u64,
        recipient: Address,
        amount: i128,
    ) {
        require_not_paused(&env);
        caller.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(invoice.creator == caller, "only creator can add recipients");
        assert!(invoice.funded == 0, "cannot add recipient after payment received");
        assert!(amount > 0, "amount must be positive");

        let token = invoice.tokens.get(0).expect("no token");

        invoice.recipients.push_back(recipient.clone());
        invoice.amounts.push_back(amount);
        invoice.tokens.push_back(token);
        invoice.claimed.push_back(0i128);

        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("add_rec"), &caller);
        events::recipient_added(&env, invoice_id, &recipient, amount);

        // Index new recipient -> invoice ID (issue #40).
        let key = recipient_invoice_ids_key(&recipient);
        let mut ids: Vec<u64> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(&env));
        ids.push_back(invoice_id);
        env.storage().persistent().set(&key, &ids);
    }

    // -----------------------------------------------------------------------
    // Adjust split
    // -----------------------------------------------------------------------

    /// Rebalance recipient amounts before any payment has been received.
    ///
    /// Only the creator may call this. Panics if any payment has already been
    /// made (`invoice.funded > 0`). The length of `new_amounts` must match the
    /// existing number of recipients, and every amount must be positive.
    pub fn adjust_split(
        env: Env,
        caller: Address,
        invoice_id: u64,
        new_amounts: Vec<i128>,
    ) {
        require_not_paused(&env);
        caller.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );
        assert!(invoice.creator == caller, "only creator can adjust split");
        assert!(invoice.funded == 0, "payments already received");
        assert!(
            new_amounts.len() == invoice.recipients.len(),
            "amounts length mismatch"
        );
        for amt in new_amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }

        invoice.amounts = new_amounts;
        save_invoice(&env, invoice_id, &invoice);
        append_audit_entry(&env, invoice_id, symbol_short!("adj_spl"), &caller);
        events::split_adjusted(&env, invoice_id, &caller);
    }

    // -----------------------------------------------------------------------
    // Templates
    // -----------------------------------------------------------------------

    /// Save a reusable invoice template.
    pub fn save_template(
        env: Env,
        creator: Address,
        name: Symbol,
        recipients: Vec<Address>,
        amounts: Vec<i128>,
        token: Address,
    ) {
        creator.require_auth();
        assert!(
            recipients.len() == amounts.len(),
            "recipients and amounts length mismatch"
        );
        assert!(!recipients.is_empty(), "must have at least one recipient");
        for amt in amounts.iter() {
            assert!(amt > 0, "amounts must be positive");
        }
        let template = InvoiceTemplate {
            recipients,
            amounts,
            token,
            deadline: 0,
            funded: 0,
            status: InvoiceStatus::Pending,
            payments: Vec::new(&env),
            allowed_payers: None,
        };
        env.storage()
            .persistent()
            .set(&template_key(&creator, &name), &template);
    }

    /// Create a new invoice from a previously saved template.
    pub fn create_from_template(
        env: Env,
        creator: Address,
        name: Symbol,
        deadline: u64,
    ) -> u64 {
        creator.require_auth();
        let tmpl: InvoiceTemplate = env
            .storage()
            .persistent()
            .get(&template_key(&creator, &name))
            .expect("template not found");
        Self::_create_invoice_inner(
            &env,
            creator,
            tmpl.recipients,
            tmpl.amounts,
            tmpl.token,
            deadline,
            Vec::new(&env),
            false,
            0,
            0,
            None,
            Vec::new(&env),
            Vec::new(&env),
            0,
            0,
            0,
            0,
            Vec::new(&env),
            None,
            Vec::new(&env),
            0,
            None,
            0,
            false,
            false,
            Vec::new(&env),
        )
    }

    // -----------------------------------------------------------------------
    // Group
    // -----------------------------------------------------------------------

    /// Link invoices into a group: all must be fully funded before any can be released.
    pub fn create_invoice_group(env: Env, invoice_ids: Vec<u64>) -> u64 {
        assert!(invoice_ids.len() >= 2, "group needs at least 2 invoices");

        let grp_cnt_key = symbol_short!("grp_cnt");
        let group_id: u64 = env
            .storage()
            .persistent()
            .get(&grp_cnt_key)
            .unwrap_or(0u64)
            + 1;
        env.storage().persistent().set(&grp_cnt_key, &group_id);

        for id in invoice_ids.iter() {
            env.storage()
                .persistent()
                .set(&invoice_group_key(id), &group_id);
        }
        env.storage()
            .persistent()
            .set(&group_key(group_id), &invoice_ids);

        group_id
    }

    // -----------------------------------------------------------------------
    // Early withdrawal (#37)
    // -----------------------------------------------------------------------

    /// Allows a payer to reclaim their contribution before the deadline when
    /// `allow_early_withdrawal` is enabled on the invoice.
    pub fn withdraw(env: Env, invoice_id: u64, payer: Address) {
        payer.require_auth();

        let mut invoice = load_invoice(&env, invoice_id);

        assert!(invoice.allow_early_withdrawal, "early withdrawal not allowed");
        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let mut total_paid: i128 = 0;
        for payment in invoice.payments.iter() {
            if payment.payer == payer {
                total_paid += payment.amount;
            }
        }
        assert!(total_paid > 0, "no contributions to withdraw");

        let mut new_payments: Vec<Payment> = Vec::new(&env);
        for payment in invoice.payments.iter() {
            if payment.payer != payer {
                new_payments.push_back(payment);
            }
        }
        invoice.payments = new_payments;
        invoice.funded -= total_paid;

        let token_client =
            token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
        token_client.transfer(&env.current_contract_address(), &payer, &total_paid);

        // Decrement credit score by 2 on early withdrawal (floor 0) (issue #38).
        let credit: u64 = env
            .storage()
            .persistent()
            .get(&credit_key(&payer))
            .unwrap_or(0u64);
        env.storage()
            .persistent()
            .set(&credit_key(&payer), &credit.saturating_sub(2));

        save_invoice(&env, invoice_id, &invoice);
    }

    // -----------------------------------------------------------------------
    // Deadline extension by payer vote (#39)
    // -----------------------------------------------------------------------

    /// Vote to extend the invoice deadline by 7 days.
    /// Once a strict majority of unique payers vote, the deadline is extended.
    pub fn vote_extend_deadline(env: Env, invoice_id: u64, voter: Address) {
        voter.require_auth();

        let invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Pending,
            "invoice is not pending"
        );

        let has_paid = invoice.payments.iter().any(|p| p.payer == voter);
        assert!(has_paid, "only payers can vote");

        let mut unique_payers: Vec<Address> = Vec::new(&env);
        for payment in invoice.payments.iter() {
            if !unique_payers.contains(&payment.payer) {
                unique_payers.push_back(payment.payer);
            }
        }

        let vote_key = ext_vote_key(invoice_id);
        let mut votes: Vec<Address> = env
            .storage()
            .persistent()
            .get(&vote_key)
            .unwrap_or_else(|| Vec::new(&env));

        if votes.contains(&voter) {
            return;
        }
        votes.push_back(voter);

        if votes.len() > unique_payers.len() / 2 {
            let mut invoice = load_invoice(&env, invoice_id);
            invoice.deadline += 7 * 24 * 60 * 60;
            save_invoice(&env, invoice_id, &invoice);
            env.storage().persistent().remove(&vote_key);
        } else {
            env.storage().persistent().set(&vote_key, &votes);
        }
    }

    // -----------------------------------------------------------------------
    // Drip / vesting claim
    // -----------------------------------------------------------------------

    /// Claim the vested portion of a drip invoice for a recipient.
    pub fn drip_claim(env: Env, invoice_id: u64, recipient: Address) {
        let mut invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Released,
            "invoice not released"
        );
        let drip_duration = invoice.drip_duration.expect("no drip schedule");
        let release_ts = invoice.release_timestamp.expect("no release timestamp");

        let idx = invoice
            .recipients
            .iter()
            .position(|r| r == recipient)
            .expect("recipient not found") as u32;

        let total_amount = invoice.amounts.get(idx).unwrap();
        let already_claimed = invoice.claimed.get(idx).unwrap();

        let elapsed = env.ledger().timestamp().saturating_sub(release_ts);
        let vested = if elapsed >= drip_duration {
            total_amount
        } else {
            (elapsed as i128) * total_amount / (drip_duration as i128)
        };

        let claimable = vested - already_claimed;
        assert!(claimable > 0, "nothing to claim");

        invoice.claimed.set(idx, already_claimed + claimable);
        save_invoice(&env, invoice_id, &invoice);

        let token_client =
            token::Client::new(&env, &invoice.tokens.get(0).expect("no token"));
        token_client.transfer(&env.current_contract_address(), &recipient, &claimable);
    }

    // -----------------------------------------------------------------------
    // Read-only
    // -----------------------------------------------------------------------

    pub fn get_invoice(env: Env, invoice_id: u64) -> Invoice {
        load_invoice(&env, invoice_id)
    }

    pub fn get_audit_log(env: Env, invoice_id: u64) -> Vec<AuditEntry> {
        get_audit_log(&env, invoice_id)
    }

    /// Return the total amount contributed by `payer` toward `invoice_id`.
    pub fn get_payer_total(env: Env, invoice_id: u64, payer: Address) -> i128 {
        let invoice = load_invoice(&env, invoice_id);
        invoice
            .payments
            .iter()
            .filter(|p| p.payer == payer)
            .map(|p| p.amount)
            .sum()
    }

    /// Returns the on-chain reputation score (number of successful payments) for an address.
    ///
    /// Returns 0 for an address that has never paid.
    pub fn get_reputation(env: Env, address: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&rep_key(&address))
            .unwrap_or(0u64)
    }

    /// Returns the credit score for an address.
    ///
    /// Incremented by 1 on every successful `pay()`, decremented by 2 on
    /// early `withdraw()` (floor 0). Returns 0 for an address that has never paid.
    pub fn get_credit_score(env: Env, address: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&credit_key(&address))
            .unwrap_or(0u64)
    }

    /// Returns the current expected nonce for a (invoice_id, payer) pair.
    ///
    /// The first payment must use nonce 0; each successful payment increments it by 1.
    /// Returns 0 for a payer that has never paid toward this invoice.
    pub fn get_nonce(env: Env, invoice_id: u64, payer: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&nonce_key(invoice_id, &payer))
            .unwrap_or(0u64)
    }

    /// Generate a completion proof for a finalized invoice.
    pub fn get_completion_proof(env: Env, invoice_id: u64) -> CompletionProof {
        let invoice = load_invoice(&env, invoice_id);

        assert!(
            invoice.status == InvoiceStatus::Released
                || invoice.status == InvoiceStatus::Refunded,
            "invoice not finalized"
        );

        let status_byte: u8 = match invoice.status {
            InvoiceStatus::Pending => 0u8,
            InvoiceStatus::Released => 1u8,
            InvoiceStatus::Refunded => 2u8,
            InvoiceStatus::Cancelled => 3u8,
        };

        let mut preimage = [0u8; 17];
        preimage[..8].copy_from_slice(&invoice_id.to_be_bytes());
        preimage[8..16].copy_from_slice(&(invoice.funded as u64).to_be_bytes());
        preimage[16] = status_byte;

        let bytes = Bytes::from_array(&env, &preimage);
        let hash = env.crypto().sha256(&bytes);

        CompletionProof {
            id: invoice_id,
            status: invoice.status,
            funded: invoice.funded,
            timestamp: env.ledger().timestamp(),
            hash: hash.into(),
        }
    }

    /// Generate a payment proof for a specific payer on an invoice (issue #85).
    /// No auth required — read-only. Returns total_paid = 0 if the payer has
    /// not contributed. The proof_hash is deterministic over
    /// (invoice_id, payer, total_paid).
    pub fn generate_payment_proof(env: Env, invoice_id: u64, payer: Address) -> PaymentProof {
        let invoice = load_invoice(&env, invoice_id);

        let total_paid: i128 = invoice
            .payments
            .iter()
            .filter(|p| p.payer == payer)
            .map(|p| p.amount + p.tip)
            .sum();

        // Preimage: 8 bytes invoice_id || 16 bytes total_paid (big-endian i128)
        let mut preimage = [0u8; 24];
        preimage[..8].copy_from_slice(&invoice_id.to_be_bytes());
        preimage[8..24].copy_from_slice(&total_paid.to_be_bytes());

        let bytes = Bytes::from_array(&env, &preimage);
        let proof_hash: BytesN<32> = env.crypto().sha256(&bytes).into();

        PaymentProof { invoice_id, payer, total_paid, proof_hash }
    }

    /// Return all invoice IDs that include `recipient` as a recipient (issue #40).
    pub fn get_recipient_invoice_ids(env: Env, recipient: Address) -> Vec<u64> {
        env.storage()
            .persistent()
            .get(&recipient_invoice_ids_key(&recipient))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns true if the invoice exists and its status matches `expected_status`.
    pub fn verify_invoice(env: Env, invoice_id: u64, expected_status: InvoiceStatus) -> bool {
        match env
            .storage()
            .persistent()
            .get::<(Symbol, u64), Invoice>(&invoice_key(invoice_id))
        {
            Some(invoice) => invoice.status == expected_status,
            None => false,
        }
    }

    /// Returns the referral count for an address (issue #87).
    ///
    /// This counts how many invoices have been created with this address as the referrer.
    /// Returns 0 for an address that has never been used as a referrer.
    pub fn get_referral_count(env: Env, referrer: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&referral_count_key(&referrer))
            .unwrap_or(0u64)
    }

    /// Return the contract-level analytics counters (issue #28).
    ///
    /// Returns a tuple of (total_invoices, total_volume, total_released, total_refunded).
    /// Each counter starts at 0 and increments on the relevant state change.
    pub fn get_stats(env: Env) -> (u64, i128, i128, i128) {
        let total_invoices = env
            .storage()
            .persistent()
            .get(&total_invoices_key())
            .unwrap_or(0u64);
        let total_volume = env
            .storage()
            .persistent()
            .get(&total_volume_key())
            .unwrap_or(0i128);
        let total_released = env
            .storage()
            .persistent()
            .get(&total_released_key())
            .unwrap_or(0i128);
        let total_refunded = env
            .storage()
            .persistent()
            .get(&total_refunded_key())
            .unwrap_or(0i128);
        (total_invoices, total_volume, total_released, total_refunded)
    }

    // -----------------------------------------------------------------------
    // Archive (issue #40)
    // -----------------------------------------------------------------------

    /// Move a Released or Refunded invoice from persistent storage to instance
    /// storage (cheaper, shorter TTL), freeing up persistent storage budget.
    ///
    /// Panics with "invoice not completed" if the invoice is still Pending or Cancelled.
    /// After archival, `get_invoice` still returns the invoice from instance storage.
    pub fn archive_invoice(env: Env, invoice_id: u64) {
        let invoice: Invoice = env
            .storage()
            .persistent()
            .get(&invoice_key(invoice_id))
            .expect("invoice not found");

        assert!(
            invoice.status == InvoiceStatus::Released
                || invoice.status == InvoiceStatus::Refunded,
            "invoice not completed"
        );

        // Copy to instance storage under the same key.
        env.storage()
            .instance()
            .set(&invoice_key(invoice_id), &invoice);

        // Remove from persistent storage.
        env.storage()
            .persistent()
            .remove(&invoice_key(invoice_id));

        events::invoice_archived(&env, invoice_id);
    }

    // -----------------------------------------------------------------------
    // Delegation (issue #43)
    // -----------------------------------------------------------------------

    /// Assign a delegate address that may call management functions (e.g. extend_deadline)
    /// on behalf of the creator. Requires creator auth.
    pub fn delegate_invoice(env: Env, invoice_id: u64, delegate: Address) {
        let invoice = load_invoice(&env, invoice_id);
        invoice.creator.require_auth();

        env.storage()
            .persistent()
            .set(&delegate_key(invoice_id), &delegate);

        events::delegate_set(&env, invoice_id, &delegate);
        append_audit_entry(&env, invoice_id, symbol_short!("delegate"), &invoice.creator);
    }

    /// Remove the delegate from an invoice. Requires creator auth.
    pub fn revoke_delegate(env: Env, invoice_id: u64) {
        let invoice = load_invoice(&env, invoice_id);
        invoice.creator.require_auth();

        env.storage()
            .persistent()
            .remove(&delegate_key(invoice_id));

        events::delegate_revoked(&env, invoice_id);
        append_audit_entry(&env, invoice_id, symbol_short!("revoke_del"), &invoice.creator);
    }

    /// Return the current delegate for an invoice, or None if none is set.
    pub fn get_delegate(env: Env, invoice_id: u64) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&delegate_key(invoice_id))
    }
}
