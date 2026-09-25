use {
    crate::common::{send, Fixture},
    anchor_lang::{
        prelude::Pubkey, solana_program::instruction::Instruction, InstructionData, ToAccountMetas,
    },
    anchor_spl::{
        associated_token::spl_associated_token_account,
        token_2022::spl_token_2022::{
            self,
            solana_zk_sdk::{
                encryption::{auth_encryption::AeKey, elgamal::ElGamalKeypair},
                zk_elgamal_proof_program::{
                    self,
                    instruction::{ContextStateInfo, ProofInstruction},
                    proof_data::{PubkeyValidityProofContext, PubkeyValidityProofData},
                    state::ProofContextState,
                },
            },
        },
    },
    litesvm::types::TransactionResult,
    solana_keypair::Keypair,
    solana_signer::Signer,
};

pub struct ConfigureFixture {
    pub mint: Fixture,
    pub owner: Keypair,
    pub ata: Pubkey,
    pub elgamal: ElGamalKeypair,
    pub aes: AeKey,
    pub proof_context: Pubkey,
}

impl ConfigureFixture {
    pub fn new() -> Self {
        let mut f = Fixture::new();
        let fee_key = ElGamalKeypair::new_rand();
        let initialize = Instruction::new_with_bytes(
            remittance_stablecoin::id(),
            &remittance_stablecoin::instruction::InitializeConfidentialMint {
                decimals: 6,
                transfer_fee_basis_points: 100,
                maximum_fee: 50_000,
                withdraw_withheld_authority_elgamal_pubkey: fee_key.pubkey().into(),
            }
            .data(),
            remittance_stablecoin::accounts::InitializeConfidentialMint {
                payer: f.payer.pubkey(),
                mint: f.mint.pubkey(),
                authority: f.authority.pubkey(),
                token_program: spl_token_2022::id(),
                system_program: anchor_lang::system_program::ID,
            }
            .to_account_metas(None),
        );
        send(
            &mut f.svm,
            &f.payer,
            &[initialize],
            &[&f.mint, &f.authority],
        )
        .unwrap();
        let owner = Keypair::new();
        f.svm.airdrop(&owner.pubkey(), 1_000_000).unwrap();
        let ata =
            spl_associated_token_account::address::get_associated_token_address_with_program_id(
                &owner.pubkey(),
                &f.mint.pubkey(),
                &spl_token_2022::id(),
            );
        // The payer can create the ATA without the owner's signature.
        let create = spl_associated_token_account::instruction::create_associated_token_account(
            &f.payer.pubkey(),
            &owner.pubkey(),
            &f.mint.pubkey(),
            &spl_token_2022::id(),
        );
        send(&mut f.svm, &f.payer, &[create], &[]).unwrap();

        // Test secrets remain off-chain; only the verified context and encrypted zero are submitted.
        let elgamal = ElGamalKeypair::new_rand();
        let aes = AeKey::new_rand();
        let proof = PubkeyValidityProofData::new(&elgamal).unwrap();
        let context = Keypair::new();
        let size = std::mem::size_of::<ProofContextState<PubkeyValidityProofContext>>();
        let create_context = solana_system_interface::instruction::create_account(
            &f.payer.pubkey(),
            &context.pubkey(),
            f.svm.minimum_balance_for_rent_exemption(size),
            size as u64,
            &zk_elgamal_proof_program::ID,
        );
        let verify = ProofInstruction::VerifyPubkeyValidity.encode_verify_proof(
            Some(ContextStateInfo {
                context_state_account: &context.pubkey(),
                context_state_authority: &owner.pubkey(),
            }),
            &proof,
        );
        send(&mut f.svm, &f.payer, &[create_context, verify], &[&context]).unwrap();
        Self {
            mint: f,
            owner,
            ata,
            elgamal,
            aes,
            proof_context: context.pubkey(),
        }
    }

    pub fn instruction(&self) -> Instruction {
        Instruction::new_with_bytes(
            remittance_stablecoin::id(),
            &remittance_stablecoin::instruction::ConfigureAccount {
                decryptable_zero_balance: self.aes.encrypt(0).to_bytes(),
                maximum_pending_balance_credit_counter: 65_536,
            }
            .data(),
            remittance_stablecoin::accounts::ConfigureAccount {
                payer: self.mint.payer.pubkey(),
                owner: self.owner.pubkey(),
                mint: self.mint.mint.pubkey(),
                token_account: self.ata,
                proof_context: self.proof_context,
                token_program: spl_token_2022::id(),
                system_program: anchor_lang::system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    pub fn configure(&mut self) -> TransactionResult {
        let ix = self.instruction();
        send(&mut self.mint.svm, &self.mint.payer, &[ix], &[&self.owner])
    }

    pub fn thaw_and_mint(&mut self, amount: u64) {
        let thaw = Instruction::new_with_bytes(
            remittance_stablecoin::id(),
            &remittance_stablecoin::instruction::ThawAccount {}.data(),
            remittance_stablecoin::accounts::ThawAccount {
                mint: self.mint.mint.pubkey(),
                token_account: self.ata,
                authority: self.mint.authority.pubkey(),
                token_program: spl_token_2022::id(),
            }
            .to_account_metas(None),
        );
        let mint_to = spl_token_2022::instruction::mint_to_checked(
            &spl_token_2022::id(),
            &self.mint.mint.pubkey(),
            &self.ata,
            &self.mint.authority.pubkey(),
            &[],
            amount,
            6,
        )
        .unwrap();
        send(
            &mut self.mint.svm,
            &self.mint.payer,
            &[thaw, mint_to],
            &[&self.mint.authority],
        )
        .unwrap();
    }
}
