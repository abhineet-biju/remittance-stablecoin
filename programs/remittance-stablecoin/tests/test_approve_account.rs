mod common;
#[path = "common/confidential.rs"]
mod confidential;

use {
    anchor_lang::{solana_program::instruction::Instruction, InstructionData, ToAccountMetas},
    anchor_spl::token_2022::spl_token_2022::{
        self,
        extension::{
            confidential_transfer::ConfidentialTransferAccount,
            confidential_transfer_fee::ConfidentialTransferFeeAmount,
            transfer_fee::TransferFeeAmount, BaseStateWithExtensions, StateWithExtensions,
        },
        state::{Account, AccountState},
    },
    common::send,
    confidential::ConfigureFixture,
    solana_keypair::Keypair,
    solana_signer::Signer,
};

fn approve_instruction(f: &ConfigureFixture) -> Instruction {
    Instruction::new_with_bytes(
        remittance_stablecoin::id(),
        &remittance_stablecoin::instruction::ApproveAccount {}.data(),
        remittance_stablecoin::accounts::ApproveAccount {
            mint: f.mint.mint.pubkey(),
            token_account: f.ata,
            authority: f.mint.authority.pubkey(),
            token_program: spl_token_2022::id(),
        }
        .to_account_metas(None),
    )
}

#[test]
fn approves_confidential_use_without_thawing_or_changing_balances_and_keys() {
    let mut f = ConfigureFixture::new();
    f.configure().unwrap();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    let mint_before = f.mint.svm.get_account(&f.mint.mint.pubkey()).unwrap();
    let ix = approve_instruction(&f);
    // Approval requires the issuer, not the token account owner.
    send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.mint.authority]).unwrap();
    let after = f.mint.svm.get_account(&f.ata).unwrap();
    let previous = StateWithExtensions::<Account>::unpack(&before.data).unwrap();
    let state = StateWithExtensions::<Account>::unpack(&after.data).unwrap();
    assert_eq!(state.base, previous.base);
    assert_eq!(state.base.state, AccountState::Frozen);
    assert_eq!(after.lamports, before.lamports);
    assert_eq!(after.data.len(), before.data.len());
    let confidential = state
        .get_extension::<ConfidentialTransferAccount>()
        .unwrap();
    let mut expected = *previous
        .get_extension::<ConfidentialTransferAccount>()
        .unwrap();
    assert!(!bool::from(expected.approved));
    expected.approved = true.into();
    assert_eq!(
        bytemuck::bytes_of(confidential),
        bytemuck::bytes_of(&expected)
    );
    assert_eq!(confidential.elgamal_pubkey, (*f.elgamal.pubkey()).into());
    assert_eq!(
        state.get_extension::<TransferFeeAmount>().unwrap(),
        previous.get_extension::<TransferFeeAmount>().unwrap()
    );
    assert_eq!(
        state
            .get_extension::<ConfidentialTransferFeeAmount>()
            .unwrap(),
        previous
            .get_extension::<ConfidentialTransferFeeAmount>()
            .unwrap()
    );
    let mint_after = f.mint.svm.get_account(&f.mint.mint.pubkey()).unwrap();
    assert_eq!(mint_after.data, mint_before.data);
    assert_eq!(mint_after.lamports, mint_before.lamports);
}

#[test]
fn approves_a_thawed_account_without_changing_its_public_funds() {
    let mut f = ConfigureFixture::new();
    f.thaw_and_mint(1_000_000);
    f.configure().unwrap();
    let ix = approve_instruction(&f);
    send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.mint.authority]).unwrap();
    let account = f.mint.svm.get_account(&f.ata).unwrap();
    let state = StateWithExtensions::<Account>::unpack(&account.data).unwrap();
    assert_eq!(state.base.amount, 1_000_000);
    assert_eq!(state.base.state, AccountState::Initialized);
    assert!(bool::from(
        state
            .get_extension::<ConfidentialTransferAccount>()
            .unwrap()
            .approved
    ));
}

#[test]
fn rejects_owner_self_approval_without_changing_the_account() {
    let mut f = ConfigureFixture::new();
    f.configure().unwrap();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    let mut ix = approve_instruction(&f);
    ix.accounts
        .iter_mut()
        .find(|a| a.pubkey == f.mint.authority.pubkey())
        .unwrap()
        .pubkey = f.owner.pubkey();
    let error = send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.owner]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("missing required signature")),
        "{error:?}"
    );
    let after = f.mint.svm.get_account(&f.ata).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
}

#[test]
fn requires_the_confidential_authority_signature() {
    let mut f = ConfigureFixture::new();
    f.configure().unwrap();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    let mut ix = approve_instruction(&f);
    ix.accounts
        .iter_mut()
        .find(|a| a.pubkey == f.mint.authority.pubkey())
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
fn rejects_unconfigured_accounts_without_adding_extensions() {
    let mut f = ConfigureFixture::new();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    let ix = approve_instruction(&f);
    let error = send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.mint.authority]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("Error: InvalidAccountData")),
        "{error:?}"
    );
    let after = f.mint.svm.get_account(&f.ata).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
}

#[test]
fn rejects_an_account_from_another_mint() {
    let mut f = ConfigureFixture::new();
    f.configure().unwrap();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    f.mint.mint = Keypair::new();
    f.mint.initialize(100).unwrap();
    let ix = approve_instruction(&f);
    let error = send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.mint.authority]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("ConstraintTokenMint")),
        "{error:?}"
    );
    assert_eq!(f.mint.svm.get_account(&f.ata).unwrap().data, before.data);
}

#[test]
fn rejects_the_legacy_token_program() {
    let mut f = ConfigureFixture::new();
    f.configure().unwrap();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    let mut ix = approve_instruction(&f);
    ix.accounts
        .iter_mut()
        .find(|a| a.pubkey == spl_token_2022::id())
        .unwrap()
        .pubkey = anchor_spl::token::ID;
    let error = send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.mint.authority]).unwrap_err();
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
fn repeated_approval_is_idempotent() {
    let mut f = ConfigureFixture::new();
    f.configure().unwrap();
    let ix = approve_instruction(&f);
    send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[ix.clone()],
        &[&f.mint.authority],
    )
    .unwrap();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    f.mint.svm.expire_blockhash();
    send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.mint.authority]).unwrap();
    let after = f.mint.svm.get_account(&f.ata).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
}

#[test]
fn uses_the_confidential_authority_after_rotation_not_the_mint_or_freeze_authority() {
    let mut f = ConfigureFixture::new();
    f.configure().unwrap();
    let new_authority = Keypair::new();
    f.mint
        .svm
        .airdrop(&new_authority.pubkey(), 1_000_000)
        .unwrap();
    let rotate = spl_token_2022::instruction::set_authority(
        &spl_token_2022::id(),
        &f.mint.mint.pubkey(),
        Some(&new_authority.pubkey()),
        spl_token_2022::instruction::AuthorityType::ConfidentialTransferMint,
        &f.mint.authority.pubkey(),
        &[],
    )
    .unwrap();
    send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[rotate],
        &[&f.mint.authority],
    )
    .unwrap();
    let before = f.mint.svm.get_account(&f.ata).unwrap();
    let mut ix = approve_instruction(&f);
    let error = send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[ix.clone()],
        &[&f.mint.authority],
    )
    .unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("missing required signature")),
        "{error:?}"
    );
    assert_eq!(f.mint.svm.get_account(&f.ata).unwrap().data, before.data);
    ix.accounts
        .iter_mut()
        .find(|a| a.pubkey == f.mint.authority.pubkey())
        .unwrap()
        .pubkey = new_authority.pubkey();
    send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&new_authority]).unwrap();
    let account = f.mint.svm.get_account(&f.ata).unwrap();
    let state = StateWithExtensions::<Account>::unpack(&account.data).unwrap();
    assert!(bool::from(
        state
            .get_extension::<ConfidentialTransferAccount>()
            .unwrap()
            .approved
    ));
}
