use soroban_sdk::{contracttype, Address, BytesN, Symbol, Vec};

/// Status of an invoice lifecycle.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum InvoiceStatus {
    /// Invoice created, awaiting full payment.
    Pending,
    /// All shares paid; funds released to recipients.
    Released,
    /// Deadline passed before full funding; payers refunded.
    Refunded,
    /// Invoice cancelled by creator before payments.
    Cancelled,
}

/// A single payment made toward an invoice.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Payment {
    /// Address of the payer.
    pub payer: Address,
    /// Amount paid in stroops (7 decimal places).
    pub amount: i128,
}

/// An audit log entry recording a state change.
#[contracttype]
#[derive(Clone, Debug)]
pub struct AuditEntry {
    /// Action type (e.g., "pay", "release", "refund").
    pub action: Symbol,
    /// Address that triggered the action.
    pub actor: Address,
    /// Ledger timestamp when the action occurred.
    pub timestamp: u64,
}

/// Parameters for creating a subscription invoice chain.
#[contracttype]
#[derive(Clone, Debug)]
pub struct SubscriptionParams {
    /// Address that created the subscription.
    pub creator: Address,
    /// Ordered list of recipient addresses.
    pub recipients: Vec<Address>,
    /// Amounts owed to each recipient (parallel to `recipients`).
    pub amounts: Vec<i128>,
    /// USDC token contract address.
    pub token: Address,
}

/// A completion proof for a finalized invoice.
#[contracttype]
#[derive(Clone, Debug)]
pub struct CompletionProof {
    /// The invoice ID.
    pub id: u64,
    /// Final status (Released or Refunded).
    pub status: InvoiceStatus,
    /// Total funded amount in stroops.
    pub funded: i128,
    /// Timestamp when the invoice was finalized.
    pub timestamp: u64,
    /// SHA-256 hash of the invoice data for verification.
    pub hash: BytesN<32>,
}

/// Parameters for a single invoice in a batch creation call (#44).
#[contracttype]
#[derive(Clone, Debug)]
pub struct CreateInvoiceParams {
    /// Ordered list of recipient addresses.
    pub recipients: Vec<Address>,
    /// Amounts owed to each recipient (parallel to `recipients`).
    pub amounts: Vec<i128>,
    /// USDC token contract address.
    pub token: Address,
    /// Unix timestamp after which unfunded invoices can be refunded.
    pub deadline: u64,
}

/// An on-chain invoice splitting payment among multiple recipients.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Invoice {
    /// Address that created the invoice.
    pub creator: Address,
    /// Ordered list of recipient addresses.
    pub recipients: Vec<Address>,
    /// Amounts owed to each recipient (parallel to `recipients`).
    pub amounts: Vec<i128>,
    /// USDC token contract address.
    pub token: Address,
    /// Unix timestamp after which unfunded invoices can be refunded.
    pub deadline: u64,
    /// Total amount collected so far.
    pub funded: i128,
    /// Current lifecycle status.
    pub status: InvoiceStatus,
    /// All payments made toward this invoice.
    pub payments: Vec<Payment>,
    /// Optional deadline by which recipients must claim their share (#45).
    /// If None, funds are pushed immediately on release (existing behaviour).
    pub claim_deadline: Option<u64>,
    /// Tracks which recipient indices have already claimed (#45).
    pub claimed: Vec<bool>,
    /// Ledger timestamp when the invoice was completed (Released/Refunded) (#46).
    pub completion_time: Option<u64>,
    /// SAC token addresses minted per recipient at creation (#47).
    /// proceeds_tokens[i] is the token representing recipient[i]'s share.
    pub proceeds_tokens: Vec<Address>,
}
