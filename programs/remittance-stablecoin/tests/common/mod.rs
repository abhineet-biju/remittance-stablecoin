use {
    anchor_lang::{
        solana_program::{instruction::Instruction, system_program},
        InstructionData, ToAccountMetas,
    },
    anchor_spl::token_2022::spl_token_2022,
    litesvm::{types::TransactionResult, LiteSVM},
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

pub struct Fixture {
    pub svm: LiteSVM,
    pub payer: Keypair,
    pub mint: Keypair,
    pub authority: Keypair,
}

impl Fixture {
    pub fn new() -> Self {
        let mut svm = LiteSVM::new();
        svm.add_program(
            remittance_stablecoin::id(),
            include_bytes!(concat!(
                env!("CARGO_TARGET_TMPDIR"),
                "/../deploy/remittance_stablecoin.so"
            )),
        )
        .unwrap();
        let payer = Keypair::new();
        let authority = Keypair::new();
        svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
        svm.airdrop(&authority.pubkey(), 1_000_000).unwrap();
        Self {
            svm,
            payer,
            mint: Keypair::new(),
            authority,
        }
    }

    pub fn instruction(&self, fee_basis_points: u16) -> Instruction {
        Instruction::new_with_bytes(
            remittance_stablecoin::id(),
            &remittance_stablecoin::instruction::InitializeMint {
                decimals: 6,
                transfer_fee_basis_points: fee_basis_points,
                maximum_fee: 50_000,
            }
            .data(),
            remittance_stablecoin::accounts::InitializeMint {
                payer: self.payer.pubkey(),
                mint: self.mint.pubkey(),
                authority: self.authority.pubkey(),
                token_program: spl_token_2022::id(),
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    pub fn initialize(&mut self, fee_basis_points: u16) -> TransactionResult {
        let ix = self.instruction(fee_basis_points);
        send(
            &mut self.svm,
            &self.payer,
            &[ix],
            &[&self.mint, &self.authority],
        )
    }
}

pub fn send(
    svm: &mut LiteSVM,
    payer: &Keypair,
    instructions: &[Instruction],
    signers: &[&Keypair],
) -> TransactionResult {
    let message =
        Message::new_with_blockhash(instructions, Some(&payer.pubkey()), &svm.latest_blockhash());
    let mut all_signers = vec![payer];
    all_signers.extend_from_slice(signers);
    let transaction =
        VersionedTransaction::try_new(VersionedMessage::Legacy(message), &all_signers).unwrap();
    svm.send_transaction(transaction)
}
