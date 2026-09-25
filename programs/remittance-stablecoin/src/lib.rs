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
}
