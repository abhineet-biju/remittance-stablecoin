use anchor_lang::{
    prelude::*,
    solana_program::program::invoke,
    system_program::{self, Transfer},
};
use anchor_spl::token_2022::{
    spl_token_2022::{
        extension::{BaseStateWithExtensions, ExtensionType, StateWithExtensions},
        state::Mint as SplMint,
    },
    Token2022,
};
use anchor_spl::token_interface::Mint;
use spl_token_metadata_interface::{instruction, state::TokenMetadata};

use crate::error::ErrorCode;

#[derive(Accounts)]
pub struct InitializeMetadata<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        mut,
        mint::authority = authority,
        mint::token_program = token_program,
        extensions::metadata_pointer::metadata_address = mint,
    )]
    pub mint: InterfaceAccount<'info, Mint>,

    /// The mint authority also becomes the metadata update authority.
    pub authority: Signer<'info>,

    pub token_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

impl<'info> InitializeMetadata<'info> {
    pub fn handler(&mut self, name: String, symbol: String, uri: String) -> Result<()> {
        let metadata = TokenMetadata {
            update_authority: Some(self.authority.key()).try_into()?,
            mint: self.mint.key(),
            name,
            symbol,
            uri,
            additional_metadata: Vec::new(),
        };

        // Release the data borrow before the CPI resizes the mint.
        let space = {
            let mint_account = self.mint.to_account_info();
            let data = mint_account.try_borrow_data()?;
            let mint = StateWithExtensions::<SplMint>::unpack(&data)?;
            require!(
                !mint
                    .get_extension_types()?
                    .contains(&ExtensionType::TokenMetadata),
                ErrorCode::MetadataAlreadyInitialized
            );
            mint.try_get_new_account_len_for_variable_len_extension(&metadata)?
        };

        // Token-2022 reallocates the mint; the payer only funds the rent shortfall.
        let additional_lamports = Rent::get()?
            .minimum_balance(space)
            .saturating_sub(self.mint.to_account_info().lamports());
        if additional_lamports > 0 {
            system_program::transfer(
                CpiContext::new(
                    self.system_program.key(),
                    Transfer {
                        from: self.payer.to_account_info(),
                        to: self.mint.to_account_info(),
                    },
                ),
                additional_lamports,
            )?;
        }

        let initialize_metadata = instruction::initialize(
            &self.token_program.key(),
            &self.mint.key(),
            &self.authority.key(),
            &self.mint.key(),
            &self.authority.key(),
            metadata.name,
            metadata.symbol,
            metadata.uri,
        );
        invoke(
            &initialize_metadata,
            &[
                self.mint.to_account_info(),
                self.authority.to_account_info(),
                self.token_program.to_account_info(),
            ],
        )?;
        Ok(())
    }
}
