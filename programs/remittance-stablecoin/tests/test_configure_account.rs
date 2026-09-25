mod common;
#[path = "common/confidential.rs"]
mod confidential;

use {
    anchor_lang::InstructionData,
    anchor_spl::token_2022::spl_token_2022::{
        self,
        extension::{
            confidential_transfer::ConfidentialTransferAccount,
            confidential_transfer_fee::ConfidentialTransferFeeAmount,
            transfer_fee::TransferFeeAmount, BaseStateWithExtensions, ExtensionType,
            StateWithExtensions,
        },
        state::{Account, AccountState},
    },
    common::send,
    confidential::ConfigureFixture,
    solana_keypair::Keypair,
    solana_signer::Signer,
    solana_zk_elgamal_proof_interface::{
        self as zk_elgamal_proof_program, proof_data::PubkeyValidityProofContext,
        state::ProofContextState,
    },
    solana_zk_sdk::encryption::auth_encryption::AeCiphertext,
};

#[test]
fn configures_frozen_account_with_zero_balances_and_fees_but_without_approval() {
    let mut f = ConfigureFixture::new();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    let mint_before = f.mint.svm.get_account(&f.mint.mint.pubkey()).unwrap().data;
    let proof_before = f.mint.svm.get_account(&f.proof_context).unwrap().data;
    f.configure().unwrap();
    let after = f.mint.svm.get_account(&f.ata).unwrap();
    assert_eq!(after.owner, spl_token_2022::id());
    assert_eq!(
        after.lamports,
        f.mint
            .svm
            .minimum_balance_for_rent_exemption(after.data.len())
    );
    assert!(after.data.len() > before.data.len());
    let state = StateWithExtensions::<Account>::unpack(&after.data).unwrap();
    let previous = StateWithExtensions::<Account>::unpack(&before.data).unwrap();
    assert_eq!(state.base, previous.base);
    assert_eq!(state.base.state, AccountState::Frozen);
    assert_eq!(
        state.get_extension::<TransferFeeAmount>().unwrap(),
        previous.get_extension::<TransferFeeAmount>().unwrap()
    );
    let confidential = state
        .get_extension::<ConfidentialTransferAccount>()
        .unwrap();
    assert!(!bool::from(confidential.approved));
    assert_eq!(
        confidential.elgamal_pubkey,
        f.elgamal.pubkey().to_bytes().into()
    );
    assert_eq!(confidential.pending_balance_lo, Default::default());
    assert_eq!(confidential.pending_balance_hi, Default::default());
    assert_eq!(confidential.available_balance, Default::default());
    assert!(bool::from(confidential.allow_confidential_credits));
    assert!(bool::from(confidential.allow_non_confidential_credits));
    assert_eq!(u64::from(confidential.pending_balance_credit_counter), 0);
    assert_eq!(
        u64::from(confidential.expected_pending_balance_credit_counter),
        0
    );
    assert_eq!(
        u64::from(confidential.actual_pending_balance_credit_counter),
        0
    );
    assert_eq!(
        u64::from(confidential.maximum_pending_balance_credit_counter),
        65_536
    );
    let balance = AeCiphertext::from_bytes(bytemuck::bytes_of(
        &confidential.decryptable_available_balance,
    ))
    .unwrap();
    assert_eq!(balance.decrypt(&f.aes), Some(0));
    assert_eq!(
        state
            .get_extension::<ConfidentialTransferFeeAmount>()
            .unwrap()
            .withheld_amount,
        Default::default()
    );
    assert_eq!(
        f.mint.svm.get_account(&f.mint.mint.pubkey()).unwrap().data,
        mint_before
    );
    assert_eq!(
        f.mint.svm.get_account(&f.proof_context).unwrap().data,
        proof_before
    );
}

#[test]
fn preserves_existing_public_funds_and_leaves_thawed_accounts_unapproved() {
    let mut f = ConfigureFixture::new();
    f.thaw_and_mint(1_000_000);
    f.configure().unwrap();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    let state = StateWithExtensions::<Account>::unpack(&before.data).unwrap();
    assert_eq!(state.base.amount, 1_000_000);
    assert_eq!(state.base.state, AccountState::Initialized);
    let confidential = state
        .get_extension::<ConfidentialTransferAccount>()
        .unwrap();
    assert!(!bool::from(confidential.approved));
}

#[test]
fn creating_an_ata_does_not_authorize_the_payer_to_configure_it() {
    let mut f = ConfigureFixture::new();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    let mut ix = f.instruction();
    ix.accounts
        .iter_mut()
        .find(|a| a.pubkey == f.owner.pubkey())
        .unwrap()
        .pubkey = f.mint.payer.pubkey();
    let error = send(&mut f.mint.svm, &f.mint.payer, &[ix], &[]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("ConstraintTokenOwner")),
        "{error:?}"
    );
    let after = f.mint.svm.get_account(&f.ata).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
}

#[test]
fn requires_the_account_owner_signature() {
    let mut f = ConfigureFixture::new();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    let mut ix = f.instruction();
    ix.accounts
        .iter_mut()
        .find(|a| a.pubkey == f.owner.pubkey())
        .unwrap()
        .is_signer = false;
    let error = send(&mut f.mint.svm, &f.mint.payer, &[ix], &[]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("AccountNotSigner")),
        "{error:?}"
    );
    assert_eq!(f.mint.svm.get_account(&f.ata).unwrap().data, before.data);
}

#[test]
fn rejects_a_proof_account_owned_by_another_program() {
    let mut f = ConfigureFixture::new();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    let mut ix = f.instruction();
    ix.accounts
        .iter_mut()
        .find(|a| a.pubkey == f.proof_context)
        .unwrap()
        .pubkey = f.owner.pubkey();
    let error = send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.owner]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("ConstraintOwner")),
        "{error:?}"
    );
    let after = f.mint.svm.get_account(&f.ata).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
}

#[test]
fn rejects_unverified_proof_context_and_rolls_back_reallocation_and_rent() {
    let mut f = ConfigureFixture::new();
    let context = Keypair::new();
    let size = std::mem::size_of::<ProofContextState<PubkeyValidityProofContext>>();
    let create = solana_system_interface::instruction::create_account(
        &f.mint.payer.pubkey(),
        &context.pubkey(),
        f.mint.svm.minimum_balance_for_rent_exemption(size),
        size as u64,
        &zk_elgamal_proof_program::ID,
    );
    send(&mut f.mint.svm, &f.mint.payer, &[create], &[&context]).unwrap();
    f.proof_context = context.pubkey();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    let error = f.configure().unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("invalid instruction data")),
        "{error:?}"
    );
    let after = f.mint.svm.get_account(&f.ata).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
}

#[test]
fn rejects_reconfiguration_without_resetting_account_state() {
    let mut f = ConfigureFixture::new();
    f.configure().unwrap();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    let error = f.configure().unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("already initialized")),
        "{error:?}"
    );
    let after = f.mint.svm.get_account(&f.ata).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
}

#[test]
fn rejects_a_mismatched_mint_before_reallocation() {
    let mut f = ConfigureFixture::new();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    f.mint.mint = Keypair::new();
    f.mint.initialize(100).unwrap();
    let error = f.configure().unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("ConstraintTokenMint")),
        "{error:?}"
    );
    let after = f.mint.svm.get_account(&f.ata).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
}

#[test]
fn rejects_the_legacy_token_program() {
    let mut f = ConfigureFixture::new();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    let mut ix = f.instruction();
    ix.accounts
        .iter_mut()
        .find(|a| a.pubkey == spl_token_2022::id())
        .unwrap()
        .pubkey = anchor_spl::token::ID;
    let error = send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.owner]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("InvalidProgramId")),
        "{error:?}"
    );
    assert_eq!(f.mint.svm.get_account(&f.ata).unwrap().data, before.data);
}

#[test]
fn stores_the_owner_selected_pending_credit_limit() {
    let mut f = ConfigureFixture::new();
    let mut ix = f.instruction();
    ix.data = remittance_stablecoin::instruction::ConfigureAccount {
        decryptable_zero_balance: f.aes.encrypt(0).to_bytes(),
        maximum_pending_balance_credit_counter: 7,
    }
    .data();
    send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.owner]).unwrap();
    let account = f.mint.svm.get_account(&f.ata).unwrap();
    let state = StateWithExtensions::<Account>::unpack(&account.data).unwrap();
    assert_eq!(
        u64::from(
            state
                .get_extension::<ConfidentialTransferAccount>()
                .unwrap()
                .maximum_pending_balance_credit_counter
        ),
        7
    );
    assert!(state
        .get_extension_types()
        .unwrap()
        .contains(&ExtensionType::ConfidentialTransferFeeAmount));
}
