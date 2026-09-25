#![allow(dead_code)] // Each instruction suite uses a different part of the lifecycle.
use {
    crate::{
        common::{send, Fixture},
        confidential::ConfigureFixture,
    },
    anchor_lang::{
        prelude::Pubkey, solana_program::instruction::Instruction, InstructionData, ToAccountMetas,
    },
    anchor_spl::token_2022::spl_token_2022::{
        self,
        extension::{
            confidential_transfer::ConfidentialTransferAccount, BaseStateWithExtensions,
            StateWithExtensions,
        },
        state::Account,
    },
    solana_keypair::Keypair,
    solana_signer::Signer,
    solana_zk_elgamal_proof_interface::{
        self as zk_elgamal_proof_program,
        instruction::{ContextStateInfo, ProofInstruction},
        proof_data::ZkProofData,
        state::ProofContextState,
    },
    solana_zk_sdk::encryption::{
        auth_encryption::{AeCiphertext, AeKey},
        elgamal::{ElGamalCiphertext, ElGamalKeypair},
    },
};

pub fn approve(f: &mut ConfigureFixture) {
    let ix = Instruction::new_with_bytes(
        remittance_stablecoin::id(),
        &remittance_stablecoin::instruction::ApproveAccount {}.data(),
        remittance_stablecoin::accounts::ApproveAccount {
            mint: f.mint.mint.pubkey(),
            token_account: f.ata,
            authority: f.mint.authority.pubkey(),
            token_program: spl_token_2022::id(),
        }
        .to_account_metas(None),
    );
    send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.mint.authority]).unwrap();
}
pub fn setup(amount: u64) -> ConfigureFixture {
    let mut f = ConfigureFixture::new();
    f.configure().unwrap();
    approve(&mut f);
    f.thaw_and_mint(amount);
    f
}
pub fn deposit_instruction(f: &ConfigureFixture, amount: u64) -> Instruction {
    Instruction::new_with_bytes(
        remittance_stablecoin::id(),
        &remittance_stablecoin::instruction::Deposit { amount }.data(),
        remittance_stablecoin::accounts::Deposit {
            owner: f.owner.pubkey(),
            mint: f.mint.mint.pubkey(),
            token_account: f.ata,
            token_program: spl_token_2022::id(),
        }
        .to_account_metas(None),
    )
}
pub fn deposit(f: &mut ConfigureFixture, amount: u64) {
    let ix = deposit_instruction(f, amount);
    send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.owner]).unwrap();
}
pub fn extension(f: &Fixture, ata: Pubkey) -> ConfidentialTransferAccount {
    let account = f.svm.get_account(&ata).unwrap();
    *StateWithExtensions::<Account>::unpack(&account.data)
        .unwrap()
        .get_extension::<ConfidentialTransferAccount>()
        .unwrap()
}
pub fn apply_instruction(
    f: &Fixture,
    owner: Pubkey,
    ata: Pubkey,
    aes: &AeKey,
    total: u64,
) -> Instruction {
    Instruction::new_with_bytes(
        remittance_stablecoin::id(),
        &remittance_stablecoin::instruction::ApplyPending {
            expected_pending_balance_credit_counter: extension(f, ata)
                .pending_balance_credit_counter
                .into(),
            new_decryptable_available_balance: aes.encrypt(total).to_bytes(),
        }
        .data(),
        remittance_stablecoin::accounts::ApplyPending {
            owner,
            mint: f.mint.pubkey(),
            token_account: ata,
            token_program: spl_token_2022::id(),
        }
        .to_account_metas(None),
    )
}
pub fn apply(f: &mut ConfigureFixture, total: u64) {
    let ix = apply_instruction(&f.mint, f.owner.pubkey(), f.ata, &f.aes, total);
    send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.owner]).unwrap();
}
pub fn balances(f: &Fixture, ata: Pubkey, key: &ElGamalKeypair, aes: &AeKey) -> (u64, u64, u64) {
    let account = f.svm.get_account(&ata).unwrap();
    let state = StateWithExtensions::<Account>::unpack(&account.data).unwrap();
    let e = state
        .get_extension::<ConfidentialTransferAccount>()
        .unwrap();
    let decrypt = |bytes| {
        ElGamalCiphertext::from_bytes(bytes)
            .unwrap()
            .decrypt_u32(key.secret())
            .unwrap()
    };
    // Fee subtraction may make an individual component negative. Combine before decrypting.
    let pending = spl_token_confidential_transfer_proof_generation::try_combine_lo_hi_ciphertexts(
        &ElGamalCiphertext::from_bytes(bytemuck::bytes_of(&e.pending_balance_lo)).unwrap(),
        &ElGamalCiphertext::from_bytes(bytemuck::bytes_of(&e.pending_balance_hi)).unwrap(),
        16,
    )
    .unwrap()
    .decrypt_u32(key.secret())
    .unwrap();
    let available = decrypt(bytemuck::bytes_of(&e.available_balance));
    let readable = AeCiphertext::from_bytes(bytemuck::bytes_of(&e.decryptable_available_balance))
        .unwrap()
        .decrypt(aes)
        .unwrap();
    assert_eq!(available, readable);
    (state.base.amount, pending, available)
}
pub fn assert_balances(f: &ConfigureFixture, public: u64, pending: u64, available: u64) {
    assert_eq!(
        balances(&f.mint, f.ata, &f.elgamal, &f.aes),
        (public, pending, available)
    );
}
pub fn stage_proof<T: bytemuck::Pod + ZkProofData<U>, U: bytemuck::Pod>(
    f: &mut Fixture,
    authority: Pubkey,
    kind: ProofInstruction,
    proof: &T,
) -> Pubkey {
    let context = Keypair::new();
    let size = std::mem::size_of::<ProofContextState<U>>();
    let create = solana_system_interface::instruction::create_account(
        &f.payer.pubkey(),
        &context.pubkey(),
        f.svm.minimum_balance_for_rent_exemption(size),
        size as u64,
        &zk_elgamal_proof_program::ID,
    );
    send(&mut f.svm, &f.payer, &[create], &[&context]).unwrap();
    let verify = kind.encode_verify_proof(
        Some(ContextStateInfo {
            context_state_account: &context.pubkey(),
            context_state_authority: &authority,
        }),
        proof,
    );
    // The U256 range proof exceeds the default 200,000 compute-unit limit.
    let budget =
        solana_compute_budget_interface::ComputeBudgetInstruction::set_compute_unit_limit(500_000);
    send(&mut f.svm, &f.payer, &[budget, verify], &[]).unwrap();
    context.pubkey()
}
pub fn close_proofs(f: &mut Fixture, owner: &Keypair, contexts: &[Pubkey]) {
    let rent: u64 = contexts
        .iter()
        .map(|key| f.svm.get_account(key).unwrap().lamports)
        .sum();
    let before = f.svm.get_balance(&owner.pubkey()).unwrap_or(0);
    let closes: Vec<_> = contexts
        .iter()
        .map(|key| {
            zk_elgamal_proof_program::instruction::close_context_state(
                ContextStateInfo {
                    context_state_account: key,
                    context_state_authority: &owner.pubkey(),
                },
                &owner.pubkey(),
            )
        })
        .collect();
    send(&mut f.svm, &f.payer, &closes, &[owner]).unwrap();
    assert_eq!(f.svm.get_balance(&owner.pubkey()).unwrap(), before + rent);
    for key in contexts {
        assert!(f.svm.get_account(key).is_none_or(|a| a.lamports == 0));
    }
}

pub struct Prepared {
    pub instruction: Instruction,
    pub contexts: Vec<Pubkey>,
}

pub fn prepare_withdraw(
    f: &mut Fixture,
    owner: &Keypair,
    ata: Pubkey,
    key: &ElGamalKeypair,
    aes: &AeKey,
    amount: u64,
) -> Prepared {
    use spl_token_confidential_transfer_proof_generation::withdraw::withdraw_proof_data;
    let e = extension(f, ata);
    assert_eq!(
        e.expected_pending_balance_credit_counter, e.actual_pending_balance_credit_counter,
        "Reconcile the AES balance before generating a spending proof"
    );
    let current = AeCiphertext::from_bytes(bytemuck::bytes_of(&e.decryptable_available_balance))
        .unwrap()
        .decrypt(aes)
        .unwrap();
    let data = withdraw_proof_data(
        &ElGamalCiphertext::from_bytes(bytemuck::bytes_of(&e.available_balance)).unwrap(),
        current,
        amount,
        key,
    )
    .unwrap();
    let equality = stage_proof(
        f,
        owner.pubkey(),
        ProofInstruction::VerifyCiphertextCommitmentEquality,
        &data.equality_proof_data,
    );
    let range = stage_proof(
        f,
        owner.pubkey(),
        ProofInstruction::VerifyBatchedRangeProofU64,
        &data.range_proof_data,
    );
    let instruction = Instruction::new_with_bytes(
        remittance_stablecoin::id(),
        &remittance_stablecoin::instruction::Withdraw {
            amount,
            new_decryptable_available_balance: aes
                .encrypt(current.checked_sub(amount).unwrap())
                .to_bytes(),
        }
        .data(),
        remittance_stablecoin::accounts::Withdraw {
            owner: owner.pubkey(),
            mint: f.mint.pubkey(),
            token_account: ata,
            equality_proof_context: equality,
            range_proof_context: range,
            token_program: spl_token_2022::id(),
        }
        .to_account_metas(None),
    );
    Prepared {
        instruction,
        contexts: vec![equality, range],
    }
}

pub struct Recipient {
    pub owner: Keypair,
    pub ata: Pubkey,
    pub elgamal: ElGamalKeypair,
    pub aes: AeKey,
}

pub fn recipient(f: &mut ConfigureFixture) -> Recipient {
    use anchor_spl::associated_token::spl_associated_token_account;
    use solana_zk_sdk::zk_elgamal_proof_program::build_pubkey_validity_proof_data;
    let owner = Keypair::new();
    let elgamal = ElGamalKeypair::new_rand();
    let aes = AeKey::new_rand();
    let proof = build_pubkey_validity_proof_data(&elgamal).unwrap();
    let context = stage_proof(
        &mut f.mint,
        owner.pubkey(),
        ProofInstruction::VerifyPubkeyValidity,
        &proof,
    );
    let ata = spl_associated_token_account::address::get_associated_token_address_with_program_id(
        &owner.pubkey(),
        &f.mint.mint.pubkey(),
        &spl_token_2022::id(),
    );
    let create = spl_associated_token_account::instruction::create_associated_token_account(
        &f.mint.payer.pubkey(),
        &owner.pubkey(),
        &f.mint.mint.pubkey(),
        &spl_token_2022::id(),
    );
    let configure = Instruction::new_with_bytes(
        remittance_stablecoin::id(),
        &remittance_stablecoin::instruction::ConfigureAccount {
            decryptable_zero_balance: aes.encrypt(0).to_bytes(),
            maximum_pending_balance_credit_counter: 65_536,
        }
        .data(),
        remittance_stablecoin::accounts::ConfigureAccount {
            payer: f.mint.payer.pubkey(),
            owner: owner.pubkey(),
            mint: f.mint.mint.pubkey(),
            token_account: ata,
            proof_context: context,
            token_program: spl_token_2022::id(),
            system_program: anchor_lang::system_program::ID,
        }
        .to_account_metas(None),
    );
    let approve = Instruction::new_with_bytes(
        remittance_stablecoin::id(),
        &remittance_stablecoin::instruction::ApproveAccount {}.data(),
        remittance_stablecoin::accounts::ApproveAccount {
            mint: f.mint.mint.pubkey(),
            token_account: ata,
            authority: f.mint.authority.pubkey(),
            token_program: spl_token_2022::id(),
        }
        .to_account_metas(None),
    );
    let thaw = Instruction::new_with_bytes(
        remittance_stablecoin::id(),
        &remittance_stablecoin::instruction::ThawAccount {}.data(),
        remittance_stablecoin::accounts::ThawAccount {
            mint: f.mint.mint.pubkey(),
            token_account: ata,
            authority: f.mint.authority.pubkey(),
            token_program: spl_token_2022::id(),
        }
        .to_account_metas(None),
    );
    send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[create, configure, approve, thaw],
        &[&owner, &f.mint.authority],
    )
    .unwrap();
    close_proofs(&mut f.mint, &owner, &[context]);
    Recipient {
        owner,
        ata,
        elgamal,
        aes,
    }
}

pub fn prepare_transfer(
    f: &mut ConfigureFixture,
    recipient: &Recipient,
    amount: u64,
    fee_override: Option<(u16, u64)>,
) -> Prepared {
    use {
        anchor_spl::token_2022::spl_token_2022::{
            extension::transfer_fee::TransferFeeConfig, state::Mint,
        },
        spl_token_confidential_transfer_proof_generation::transfer_with_fee::transfer_with_fee_split_proof_data,
    };
    let e = extension(&f.mint, f.ata);
    assert_eq!(
        e.expected_pending_balance_credit_counter, e.actual_pending_balance_credit_counter,
        "Reconcile the AES balance before generating a spending proof"
    );
    let account = f.mint.svm.get_account(&f.mint.mint.pubkey()).unwrap();
    let mint = StateWithExtensions::<Mint>::unpack(&account.data).unwrap();
    let epoch = f.mint.svm.get_sysvar::<anchor_lang::prelude::Clock>().epoch;
    let fee = mint
        .get_extension::<TransferFeeConfig>()
        .unwrap()
        .get_epoch_fee(epoch);
    let (rate, cap) =
        fee_override.unwrap_or((fee.transfer_fee_basis_points.into(), fee.maximum_fee.into()));
    let current =
        AeCiphertext::from_bytes(bytemuck::bytes_of(&e.decryptable_available_balance)).unwrap();
    let data = transfer_with_fee_split_proof_data(
        &ElGamalCiphertext::from_bytes(bytemuck::bytes_of(&e.available_balance)).unwrap(),
        &current,
        amount,
        &f.elgamal,
        &f.aes,
        recipient.elgamal.pubkey(),
        None,
        f.fee_key.pubkey(),
        rate,
        cap,
    )
    .unwrap();
    let owner = f.owner.pubkey();
    let equality = stage_proof(
        &mut f.mint,
        owner,
        ProofInstruction::VerifyCiphertextCommitmentEquality,
        &data.equality_proof_data,
    );
    let validity = stage_proof(
        &mut f.mint,
        owner,
        ProofInstruction::VerifyBatchedGroupedCiphertext3HandlesValidity,
        &data
            .transfer_amount_ciphertext_validity_proof_data_with_ciphertext
            .proof_data,
    );
    let fee_sigma = stage_proof(
        &mut f.mint,
        owner,
        ProofInstruction::VerifyPercentageWithCap,
        &data.percentage_with_cap_proof_data,
    );
    let fee_validity = stage_proof(
        &mut f.mint,
        owner,
        ProofInstruction::VerifyBatchedGroupedCiphertext2HandlesValidity,
        &data.fee_ciphertext_validity_proof_data,
    );
    let range = stage_proof(
        &mut f.mint,
        owner,
        ProofInstruction::VerifyBatchedRangeProofU256,
        &data.range_proof_data,
    );
    let instruction = Instruction::new_with_bytes(
        remittance_stablecoin::id(),
        &remittance_stablecoin::instruction::TransferConfidential {
            new_source_decryptable_available_balance: f
                .aes
                .encrypt(
                    current
                        .decrypt(&f.aes)
                        .unwrap()
                        .checked_sub(amount)
                        .unwrap(),
                )
                .to_bytes(),
            transfer_amount_auditor_ciphertext_lo: bytemuck::bytes_of(
                &data
                    .transfer_amount_ciphertext_validity_proof_data_with_ciphertext
                    .ciphertext_lo,
            )
            .try_into()
            .unwrap(),
            transfer_amount_auditor_ciphertext_hi: bytemuck::bytes_of(
                &data
                    .transfer_amount_ciphertext_validity_proof_data_with_ciphertext
                    .ciphertext_hi,
            )
            .try_into()
            .unwrap(),
        }
        .data(),
        remittance_stablecoin::accounts::TransferConfidential {
            owner,
            mint: f.mint.mint.pubkey(),
            source: f.ata,
            destination: recipient.ata,
            equality_proof_context: equality,
            ciphertext_validity_proof_context: validity,
            fee_sigma_proof_context: fee_sigma,
            fee_ciphertext_validity_proof_context: fee_validity,
            range_proof_context: range,
            token_program: spl_token_2022::id(),
        }
        .to_account_metas(None),
    );
    Prepared {
        instruction,
        contexts: vec![equality, validity, fee_sigma, fee_validity, range],
    }
}

pub fn withheld_fee(f: &ConfigureFixture, ata: Pubkey) -> u64 {
    use anchor_spl::token_2022::spl_token_2022::extension::confidential_transfer_fee::ConfidentialTransferFeeAmount;
    let account = f.mint.svm.get_account(&ata).unwrap();
    let state = StateWithExtensions::<Account>::unpack(&account.data).unwrap();
    let fee = state
        .get_extension::<ConfidentialTransferFeeAmount>()
        .unwrap();
    ElGamalCiphertext::from_bytes(bytemuck::bytes_of(&fee.withheld_amount))
        .unwrap()
        .decrypt_u32(f.fee_key.secret())
        .unwrap()
}
