use soroban_sdk::{symbol_short, Address, Env, Vec, String};

/// Emitted when a new invoice is created.
/// Topics: (split, created, invoice_id)
/// Data: (creator, total)
pub fn invoice_created(env: &Env, invoice_id: u64, creator: &Address, total: i128, cross_chain_ref: &Option<String>) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("created"), invoice_id),
        (creator.clone(), total, cross_chain_ref.clone()),
    );
}

/// Emitted when a payment is received toward an invoice.
/// Topics: (split, paid, invoice_id)
/// Data: (payer, amount)
pub fn payment_received(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("paid"), invoice_id),
        (payer.clone(), amount),
    );
}

/// Emitted when an invoice is fully funded and funds are released.
/// Topics: (split, released, invoice_id)
/// Data: recipients
pub fn invoice_released(env: &Env, invoice_id: u64, recipients: &Vec<Address>) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("released"), invoice_id),
        recipients.clone(),
    );
}

/// Emitted when an invoice is refunded after deadline.
/// Topics: (split, refunded, invoice_id)
/// Data: ()
pub fn invoice_refunded(env: &Env, invoice_id: u64) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("refunded"), invoice_id),
        (),
    );
}

/// Emitted once per payer when their refund is transferred.
/// Topics: (split, pay_ref, invoice_id)
/// Data: (payer, amount)
pub fn payer_refunded(env: &Env, invoice_id: u64, payer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("pay_ref"), invoice_id),
        (payer.clone(), amount),
    );
}

/// Emitted when a recipient is added to a pending invoice.
/// Topics: (split, add_rec, invoice_id)
/// Data: (recipient, amount)
pub fn recipient_added(env: &Env, invoice_id: u64, recipient: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("add_rec"), invoice_id),
        (recipient.clone(), amount),
    );
}

/// Emitted when the creator adjusts recipient split amounts.
/// Topics: (split, adj_spl, invoice_id)
/// Data: creator
pub fn split_adjusted(env: &Env, invoice_id: u64, creator: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("adj_spl"), invoice_id),
        creator.clone(),
    );
}

/// Emitted when an invoice is archived to instance storage.
/// Topics: (split, archived, invoice_id)
/// Data: ()
pub fn invoice_archived(env: &Env, invoice_id: u64) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("archived"), invoice_id),
        (),
    );
}

/// Emitted when a delegate is assigned to an invoice.
/// Topics: (split, delegated, invoice_id)
/// Data: delegate
pub fn delegate_set(env: &Env, invoice_id: u64, delegate: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("delegated"), invoice_id),
        delegate.clone(),
    );
}

/// Emitted when a delegate is revoked from an invoice.
/// Topics: (split, revoked, invoice_id)
/// Data: ()
pub fn delegate_revoked(env: &Env, invoice_id: u64) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("revoked"), invoice_id),
        (),
    );
}

/// Emitted when an invoice is partially released.
/// Topics: (split, part_rel, invoice_id)
/// Data: recipients
pub fn invoice_partially_released(env: &Env, invoice_id: u64, recipients: &Vec<Address>) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("part_rel"), invoice_id),
        recipients.clone(),
    );
}

/// Emitted when a payment reminder is triggered.
/// Topics: (split, reminder, invoice_id)
/// Data: who
pub fn payment_reminder(env: &Env, invoice_id: u64, who: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("reminder"), invoice_id),
        who.clone(),
    );
}

/// Emitted when a payment is matched via memo.
/// Topics: (split, matched, invoice_id)
/// Data: (payer, memo)
pub fn payment_matched(env: &Env, invoice_id: u64, memo: u64, payer: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("matched"), invoice_id),
        (memo, payer.clone()),
    );
}

/// Emitted when an invoice is cloned.
/// Topics: (cloned, source_id, new_id)
/// Data: ()
pub fn invoice_cloned(env: &Env, source_id: u64, new_id: u64) {
    env.events().publish(
        (symbol_short!("cloned"), source_id, new_id),
        (),
    );
}

/// Emitted when a creator pauses an invoice.
/// Topics: (split, paused, invoice_id)
/// Data: (creator, reason, auto_resume_at)
pub fn invoice_paused(env: &Env, invoice_id: u64, creator: &Address, reason: &String, auto_resume_at: &Option<u64>) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("paused"), invoice_id),
        (creator.clone(), reason.clone(), auto_resume_at.clone()),
    );
}

/// Emitted when a creator resumes a paused invoice.
/// Topics: (split, resumed, invoice_id)
/// Data: creator
pub fn invoice_resumed(env: &Env, invoice_id: u64, creator: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("resumed"), invoice_id),
        creator.clone(),
    );
}

/// Emitted when an admin force-resumes a paused invoice.
/// Topics: (split, frc_rsm, invoice_id)
/// Data: admin
pub fn invoice_force_resumed(env: &Env, invoice_id: u64, admin: &Address) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("frc_rsm"), invoice_id),
        admin.clone(),
    );
}

/// Emitted when a creator issues a voluntary partial refund.
/// Topics: (split, part_ref, invoice_id)
/// Data: (creator, bps, total_refunded)
pub fn partial_refund_issued(env: &Env, invoice_id: u64, creator: &Address, bps: u32, total_refunded: i128) {
    env.events().publish(
        (symbol_short!("split"), symbol_short!("part_ref"), invoice_id),
        (creator.clone(), bps, total_refunded),
    );
}
