pub mod error;
pub mod instructions;

use anchor_lang::prelude::*;
pub use instructions::*;

declare_id!("CWmj86ohTg2kz1FnmEmHB8EBRuBxc9UQRHTnu4NV7Ygt");

#[program]
pub mod remittance_stablecoin {
    use super::*;

    /// Creates a Token-2022 mint with transfer fees and frozen accounts by default.
    pub fn initialize_mint(
        ctx: Context<InitializeMint>,
        decimals: u8,
        transfer_fee_basis_points: u16,
        maximum_fee: u64,
    ) -> Result<()> {
        ctx.accounts
            .handler(decimals, transfer_fee_basis_points, maximum_fee)
    }

    /// Stores the token name, symbol, and URI on the mint itself.
    pub fn initialize_metadata(
        ctx: Context<InitializeMetadata>,
        name: String,
        symbol: String,
        uri: String,
    ) -> Result<()> {
        ctx.accounts.handler(name, symbol, uri)
    }

    /// Lets the freeze authority approve an individual token account after KYC.
    pub fn thaw_account(ctx: Context<ThawAccount>) -> Result<()> {
        ctx.accounts.handler()
    }

    /// Transfers public tokens using the mint's current epoch fee configuration.
    pub fn transfer(ctx: Context<Transfer>, amount: u64) -> Result<()> {
        ctx.accounts.handler(amount)
    }

    /// Creates a new mint with manual confidential approval, encrypted fees, and a permanent delegate.
    pub fn initialize_confidential_mint(
        ctx: Context<InitializeConfidentialMint>,
        decimals: u8,
        transfer_fee_basis_points: u16,
        maximum_fee: u64,
        withdraw_withheld_authority_elgamal_pubkey: [u8; 32],
    ) -> Result<()> {
        ctx.accounts.handler(
            decimals,
            transfer_fee_basis_points,
            maximum_fee,
            withdraw_withheld_authority_elgamal_pubkey,
        )
    }

    /// Configures an existing account for confidential balances with its owner's signature.
    pub fn configure_account(
        ctx: Context<ConfigureAccount>,
        decryptable_zero_balance: [u8; 36],
        maximum_pending_balance_credit_counter: u64,
    ) -> Result<()> {
        ctx.accounts.handler(
            decryptable_zero_balance,
            maximum_pending_balance_credit_counter,
        )
    }

    /// Lets the confidential-transfer authority approve an owner-configured account.
    pub fn approve_account(ctx: Context<ApproveAccount>) -> Result<()> {
        ctx.accounts.handler()
    }

    /// Moves public tokens into confidential pending balance.
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        ctx.accounts.handler(amount)
    }

    /// Consolidates pending funds using the owner's encrypted new available balance.
    pub fn apply_pending(
        ctx: Context<ApplyPending>,
        expected_pending_balance_credit_counter: u64,
        new_decryptable_available_balance: [u8; 36],
    ) -> Result<()> {
        ctx.accounts.handler(
            expected_pending_balance_credit_counter,
            new_decryptable_available_balance,
        )
    }

    /// Withdraws available confidential funds into public balance.
    pub fn withdraw(
        ctx: Context<Withdraw>,
        amount: u64,
        new_decryptable_available_balance: [u8; 36],
    ) -> Result<()> {
        ctx.accounts
            .handler(amount, new_decryptable_available_balance)
    }

    /// Transfers encrypted funds and withholds the mint's fee using verified proof contexts.
    pub fn transfer_confidential(
        ctx: Context<TransferConfidential>,
        new_source_decryptable_available_balance: [u8; 36],
        transfer_amount_auditor_ciphertext_lo: [u8; 64],
        transfer_amount_auditor_ciphertext_hi: [u8; 64],
    ) -> Result<()> {
        ctx.accounts.handler(
            new_source_decryptable_available_balance,
            transfer_amount_auditor_ciphertext_lo,
            transfer_amount_auditor_ciphertext_hi,
        )
    }
}
