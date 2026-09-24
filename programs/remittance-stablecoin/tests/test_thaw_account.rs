mod common;

use {
    anchor_lang::{
        prelude::Pubkey, solana_program::instruction::Instruction, InstructionData, ToAccountMetas,
    },
    anchor_spl::{
        associated_token::spl_associated_token_account,
        token_2022::spl_token_2022::{
            self,
            extension::{
                default_account_state::DefaultAccountState, BaseStateWithExtensions,
                StateWithExtensions,
            },
            state::{Account, AccountState, Mint},
        },
    },
    common::{send, Fixture},
    solana_keypair::Keypair,
    solana_signer::Signer,
};

fn create_account(f: &mut Fixture) -> Pubkey {
    let owner = Keypair::new().pubkey();
    let ata = spl_associated_token_account::address::get_associated_token_address_with_program_id(
        &owner,
        &f.mint.pubkey(),
        &spl_token_2022::id(),
    );
    let ix = spl_associated_token_account::instruction::create_associated_token_account(
        &f.payer.pubkey(),
        &owner,
        &f.mint.pubkey(),
        &spl_token_2022::id(),
    );
    send(&mut f.svm, &f.payer, &[ix], &[]).unwrap();
    ata
}

fn thaw_instruction(f: &Fixture, token_account: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        remittance_stablecoin::id(),
        &remittance_stablecoin::instruction::ThawAccount {}.data(),
        remittance_stablecoin::accounts::ThawAccount {
            mint: f.mint.pubkey(),
            token_account,
            authority: f.authority.pubkey(),
            token_program: spl_token_2022::id(),
        }
        .to_account_metas(None),
    )
}

fn assert_frozen(f: &Fixture, address: Pubkey) {
    let account = f.svm.get_account(&address).unwrap();
    let state = StateWithExtensions::<Account>::unpack(&account.data).unwrap();
    assert_eq!(state.base.state, AccountState::Frozen);
}

#[test]
fn thaws_only_the_approved_account_and_keeps_existing_and_future_accounts_frozen() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    let approved = create_account(&mut f);
    let other = create_account(&mut f);
    assert_frozen(&f, approved);
    assert_frozen(&f, other);
    let mint_before = f.svm.get_account(&f.mint.pubkey()).unwrap();
    let before = f.svm.get_account(&approved).unwrap();
    let other_before = f.svm.get_account(&other).unwrap();
    let ix = thaw_instruction(&f, approved);
    // Only the freeze authority signs; the account owner does not participate.
    send(&mut f.svm, &f.payer, &[ix], &[&f.authority]).unwrap();
    let after = f.svm.get_account(&approved).unwrap();
    let state = StateWithExtensions::<Account>::unpack(&after.data).unwrap();
    let previous = StateWithExtensions::<Account>::unpack(&before.data).unwrap();
    let mut expected = previous.base;
    expected.state = AccountState::Initialized;
    assert_eq!(state.base, expected);
    assert_eq!(state.get_tlv_data(), previous.get_tlv_data());
    assert_eq!(after.lamports, before.lamports);
    assert_eq!(f.svm.get_account(&other).unwrap().data, other_before.data);
    assert_frozen(&f, other);
    let new_account = create_account(&mut f);
    assert_frozen(&f, new_account);
    let mint_after = f.svm.get_account(&f.mint.pubkey()).unwrap();
    assert_eq!(mint_after.data, mint_before.data);
    assert_eq!(mint_after.lamports, mint_before.lamports);
    let mint = StateWithExtensions::<Mint>::unpack(&mint_after.data).unwrap();
    assert_eq!(
        mint.get_extension::<DefaultAccountState>().unwrap().state,
        AccountState::Frozen as u8
    );
}

#[test]
fn rejects_a_signer_who_is_not_the_freeze_authority() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    let ata = create_account(&mut f);
    let before = f.svm.get_account(&ata).unwrap();
    let mut ix = thaw_instruction(&f, ata);
    ix.accounts
        .iter_mut()
        .find(|a| a.pubkey == f.authority.pubkey())
        .unwrap()
        .pubkey = f.payer.pubkey();
    let error = send(&mut f.svm, &f.payer, &[ix], &[]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("ConstraintMintFreezeAuthority")),
        "{error:?}"
    );
    assert_eq!(f.svm.get_account(&ata).unwrap().data, before.data);
}

#[test]
fn requires_the_freeze_authority_signature() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    let ata = create_account(&mut f);
    let before = f.svm.get_account(&ata).unwrap();
    let mut ix = thaw_instruction(&f, ata);
    ix.accounts
        .iter_mut()
        .find(|a| a.pubkey == f.authority.pubkey())
        .unwrap()
        .is_signer = false;
    let error = send(&mut f.svm, &f.payer, &[ix], &[]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("AccountNotSigner")),
        "{error:?}"
    );
    assert_eq!(f.svm.get_account(&ata).unwrap().data, before.data);
}

#[test]
fn rejects_a_token_account_from_another_mint() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    let ata = create_account(&mut f);
    let before = f.svm.get_account(&ata).unwrap();
    // Create a second real mint under the same authority to isolate the mint mismatch.
    f.mint = Keypair::new();
    f.initialize(100).unwrap();
    let ix = thaw_instruction(&f, ata);
    let error = send(&mut f.svm, &f.payer, &[ix], &[&f.authority]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("ConstraintTokenMint")),
        "{error:?}"
    );
    assert_eq!(f.svm.get_account(&ata).unwrap().data, before.data);
}

#[test]
fn rejects_the_legacy_token_program() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    let ata = create_account(&mut f);
    let before = f.svm.get_account(&ata).unwrap();
    let mut ix = thaw_instruction(&f, ata);
    ix.accounts
        .iter_mut()
        .find(|a| a.pubkey == spl_token_2022::id())
        .unwrap()
        .pubkey = anchor_spl::token::ID;
    let error = send(&mut f.svm, &f.payer, &[ix], &[&f.authority]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("InvalidProgramId")),
        "{error:?}"
    );
    assert_eq!(f.svm.get_account(&ata).unwrap().data, before.data);
}

#[test]
fn rejects_an_already_thawed_account_without_changing_it() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    let ata = create_account(&mut f);
    let ix = thaw_instruction(&f, ata);
    send(&mut f.svm, &f.payer, &[ix.clone()], &[&f.authority]).unwrap();
    let before = f.svm.get_account(&ata).unwrap();
    f.svm.expire_blockhash();
    let error = send(&mut f.svm, &f.payer, &[ix], &[&f.authority]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("Invalid account state")),
        "{error:?}"
    );
    let after = f.svm.get_account(&ata).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
}
