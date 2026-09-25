use anchor_lang::{prelude::*, solana_program::program::invoke};
use anchor_spl::{
    token_2022::{spl_token_2022::extension::confidential_transfer, Token2022},
    token_interface::{Mint, TokenAccount},
};

#[derive(Accounts)]
pub struct ApproveAccount<'info> {
    #[account(mint::token_program = token_program)]
    pub mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        token::mint = mint,
        token::token_program = token_program,
    )]
    pub token_account: InterfaceAccount<'info, TokenAccount>,

    /// Token-2022 checks this signer against the confidential-transfer mint authority.
    pub authority: Signer<'info>,

    pub token_program: Program<'info, Token2022>,
}

impl<'info> ApproveAccount<'info> {
    /// Approves confidential use without thawing the account or changing its balances.
    pub fn handler(&mut self) -> Result<()> {
        let approve = confidential_transfer::instruction::approve_account(
            &self.token_program.key(),
            &self.token_account.key(),
            &self.mint.key(),
            &self.authority.key(),
            &[],
        )?;
        invoke(
            &approve,
            &[
                self.token_account.to_account_info(),
                self.mint.to_account_info(),
                self.authority.to_account_info(),
                self.token_program.to_account_info(),
            ],
        )?;
        Ok(())
    }
}
