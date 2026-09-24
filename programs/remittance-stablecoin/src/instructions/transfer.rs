use anchor_lang::{prelude::*, solana_program::program::invoke};
use anchor_spl::{
    token_2022::{
        spl_token_2022::{
            extension::{
                transfer_fee::{self, TransferFeeConfig},
                BaseStateWithExtensions, StateWithExtensions,
            },
            state::Mint as SplMint,
        },
        Token2022,
    },
    token_interface::{Mint, TokenAccount},
};

use crate::error::ErrorCode;

#[derive(Accounts)]
pub struct Transfer<'info> {
    pub owner: Signer<'info>,

    #[account(mint::token_program = token_program)]
    pub mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        token::mint = mint,
        token::authority = owner,
        token::token_program = token_program,
    )]
    pub source: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        token::mint = mint,
        token::token_program = token_program,
    )]
    pub destination: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Program<'info, Token2022>,
}

impl<'info> Transfer<'info> {
    pub fn handler(&mut self, amount: u64) -> Result<()> {
        let current_epoch = Clock::get()?.epoch;
        let fee = {
            let mint_account = self.mint.to_account_info();
            let data = mint_account.try_borrow_data()?;
            let mint = StateWithExtensions::<SplMint>::unpack(&data)?;
            mint.get_extension::<TransferFeeConfig>()?
                .calculate_epoch_fee(current_epoch, amount)
                .ok_or(ErrorCode::FeeCalculationFailed)?
        };

        let transfer = transfer_fee::instruction::transfer_checked_with_fee(
            &self.token_program.key(),
            &self.source.key(),
            &self.mint.key(),
            &self.destination.key(),
            &self.owner.key(),
            &[],
            amount,
            self.mint.decimals,
            fee,
        )?;
        invoke(
            &transfer,
            &[
                self.source.to_account_info(),
                self.mint.to_account_info(),
                self.destination.to_account_info(),
                self.owner.to_account_info(),
                self.token_program.to_account_info(),
            ],
        )?;
        Ok(())
    }
}
