use anchor_lang::{prelude::*, solana_program::program::invoke};
use anchor_spl::token_2022::spl_token_2022::solana_zk_sdk::zk_elgamal_proof_program;
use anchor_spl::{
    token_2022::{
        spl_token_2022::extension::confidential_transfer::{
            self, DecryptableBalance, EncryptedBalance,
        },
        Token2022,
    },
    token_interface::{Mint, TokenAccount},
};
use spl_token_confidential_transfer_proof_extraction::instruction::ProofLocation;

#[derive(Accounts)]
pub struct TransferConfidential<'info> {
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

    /// CHECK: Token-2022 validates the equality proof against the source balance.
    #[account(owner = zk_elgamal_proof_program::ID)]
    pub equality_proof_context: UncheckedAccount<'info>,
    /// CHECK: Token-2022 validates transfer ciphertexts against the sender and recipient keys.
    #[account(owner = zk_elgamal_proof_program::ID)]
    pub ciphertext_validity_proof_context: UncheckedAccount<'info>,
    /// CHECK: Token-2022 validates the U256 range proof against the transfer commitments.
    #[account(owner = zk_elgamal_proof_program::ID)]
    pub range_proof_context: UncheckedAccount<'info>,

    /// CHECK: Token-2022 verifies the percentage-with-cap proof against the active fee schedule.
    #[account(owner = zk_elgamal_proof_program::ID)]
    pub fee_sigma_proof_context: UncheckedAccount<'info>,
    /// CHECK: Token-2022 verifies fee ciphertext validity for the recipient and fee authority.
    #[account(owner = zk_elgamal_proof_program::ID)]
    pub fee_ciphertext_validity_proof_context: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token2022>,
}

impl<'info> TransferConfidential<'info> {
    pub fn handler(
        &mut self,
        new_source_decryptable_available_balance: [u8; 36],
        transfer_amount_auditor_ciphertext_lo: [u8; 64],
        transfer_amount_auditor_ciphertext_hi: [u8; 64],
    ) -> Result<()> {
        let transfer = confidential_transfer::instruction::inner_transfer_with_fee(
            &self.token_program.key(),
            &self.source.key(),
            &self.mint.key(),
            &self.destination.key(),
            &DecryptableBalance::from(new_source_decryptable_available_balance),
            &EncryptedBalance::from(transfer_amount_auditor_ciphertext_lo),
            &EncryptedBalance::from(transfer_amount_auditor_ciphertext_hi),
            &self.owner.key(),
            &[],
            ProofLocation::ContextStateAccount(&self.equality_proof_context.key()),
            ProofLocation::ContextStateAccount(&self.ciphertext_validity_proof_context.key()),
            ProofLocation::ContextStateAccount(&self.fee_sigma_proof_context.key()),
            ProofLocation::ContextStateAccount(&self.fee_ciphertext_validity_proof_context.key()),
            ProofLocation::ContextStateAccount(&self.range_proof_context.key()),
        )?;
        invoke(
            &transfer,
            &[
                self.source.to_account_info(),
                self.mint.to_account_info(),
                self.destination.to_account_info(),
                self.equality_proof_context.to_account_info(),
                self.ciphertext_validity_proof_context.to_account_info(),
                self.fee_sigma_proof_context.to_account_info(),
                self.fee_ciphertext_validity_proof_context.to_account_info(),
                self.range_proof_context.to_account_info(),
                self.owner.to_account_info(),
                self.token_program.to_account_info(),
            ],
        )?;
        Ok(())
    }
}
