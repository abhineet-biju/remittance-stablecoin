use anchor_lang::prelude::*;
use anchor_spl::{
    token_2022::{self, Token2022},
    token_interface::{Mint, TokenAccount},
};

#[derive(Accounts)]
pub struct ThawAccount<'info> {
    #[account(
        mint::freeze_authority = authority,
        mint::token_program = token_program,
    )]
    pub mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        token::mint = mint,
        token::token_program = token_program,
    )]
    pub token_account: InterfaceAccount<'info, TokenAccount>,

    pub authority: Signer<'info>,

    pub token_program: Program<'info, Token2022>,
}

impl<'info> ThawAccount<'info> {
    /// Thaws one account after KYC without changing the mint's frozen default.
    pub fn handler(&mut self) -> Result<()> {
        token_2022::thaw_account(CpiContext::new(
            self.token_program.key(),
            token_2022::ThawAccount {
                account: self.token_account.to_account_info(),
                mint: self.mint.to_account_info(),
                authority: self.authority.to_account_info(),
            },
        ))
    }
}
