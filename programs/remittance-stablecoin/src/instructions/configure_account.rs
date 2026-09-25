use anchor_lang::{prelude::*, solana_program::program::invoke};
use anchor_spl::{
    token_2022::{
        spl_token_2022::{
            extension::{
                confidential_transfer::{self, DecryptableBalance},
                ExtensionType,
            },
            instruction,
            solana_zk_sdk::zk_elgamal_proof_program,
        },
        Token2022,
    },
    token_interface::{Mint, TokenAccount},
};
use spl_token_confidential_transfer_proof_extraction::instruction::ProofLocation;

#[derive(Accounts)]
pub struct ConfigureAccount<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub owner: Signer<'info>,

    #[account(mint::token_program = token_program)]
    pub mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        token::mint = mint,
        token::authority = owner,
        token::token_program = token_program,
    )]
    pub token_account: InterfaceAccount<'info, TokenAccount>,

    /// CHECK: Token-2022 validates the verified PubkeyValidity proof context and its type.
    #[account(owner = zk_elgamal_proof_program::ID)]
    pub proof_context: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

impl<'info> ConfigureAccount<'info> {
    /// The owner supplies a verified public-key proof and an off-chain AES encryption of zero.
    pub fn handler(
        &mut self,
        decryptable_zero_balance: [u8; 36],
        maximum_pending_balance_credit_counter: u64,
    ) -> Result<()> {
        let reallocate = instruction::reallocate(
            &self.token_program.key(),
            &self.token_account.key(),
            &self.payer.key(),
            &self.owner.key(),
            &[],
            &[
                ExtensionType::ConfidentialTransferAccount,
                ExtensionType::ConfidentialTransferFeeAmount,
            ],
        )?;
        invoke(
            &reallocate,
            &[
                self.token_account.to_account_info(),
                self.payer.to_account_info(),
                self.system_program.to_account_info(),
                self.owner.to_account_info(),
                self.token_program.to_account_info(),
            ],
        )?;

        let configure = confidential_transfer::instruction::inner_configure_account(
            &self.token_program.key(),
            &self.token_account.key(),
            &self.mint.key(),
            &DecryptableBalance::from(decryptable_zero_balance),
            maximum_pending_balance_credit_counter,
            &self.owner.key(),
            &[],
            ProofLocation::ContextStateAccount(&self.proof_context.key()),
        )?;
        invoke(
            &configure,
            &[
                self.token_account.to_account_info(),
                self.mint.to_account_info(),
                self.proof_context.to_account_info(),
                self.owner.to_account_info(),
                self.token_program.to_account_info(),
            ],
        )?;
        Ok(())
    }
}
