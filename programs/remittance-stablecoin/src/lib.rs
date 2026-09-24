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
}
