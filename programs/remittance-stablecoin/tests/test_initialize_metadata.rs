mod common;

use {
    anchor_lang::{
        prelude::Pubkey, solana_program::instruction::Instruction, InstructionData, ToAccountMetas,
    },
    anchor_spl::token_2022::spl_token_2022::{
        self,
        extension::{metadata_pointer, BaseStateWithExtensions, StateWithExtensions},
        state::Mint,
    },
    common::{send, Fixture},
    solana_keypair::Keypair,
    solana_signer::Signer,
    spl_token_metadata_interface::state::TokenMetadata,
};

fn metadata_instruction(f: &Fixture, name: &str, symbol: &str, uri: &str) -> Instruction {
    Instruction::new_with_bytes(
        remittance_stablecoin::id(),
        &remittance_stablecoin::instruction::InitializeMetadata {
            name: name.into(),
            symbol: symbol.into(),
            uri: uri.into(),
        }
        .data(),
        remittance_stablecoin::accounts::InitializeMetadata {
            payer: f.payer.pubkey(),
            mint: f.mint.pubkey(),
            authority: f.authority.pubkey(),
            token_program: spl_token_2022::id(),
            system_program: anchor_lang::system_program::ID,
        }
        .to_account_metas(None),
    )
}

#[test]
fn stores_metadata_on_the_mint_and_funds_rent_without_changing_existing_extensions() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    let before = f.svm.get_account(&f.mint.pubkey()).unwrap();
    let name = "Remittance ₹";
    let symbol = "RUSD";
    let uri = "https://example.com/remittance.json";
    let ix = metadata_instruction(&f, name, symbol, uri);
    send(&mut f.svm, &f.payer, &[ix], &[&f.authority]).unwrap();

    let after = f.svm.get_account(&f.mint.pubkey()).unwrap();
    assert_eq!(after.owner, spl_token_2022::id());
    assert!(after.data.len() > before.data.len());
    // The existing base mint and four extensions must remain byte-for-byte intact.
    assert_eq!(&after.data[..before.data.len()], &before.data);
    assert_eq!(
        after.lamports,
        f.svm.minimum_balance_for_rent_exemption(after.data.len())
    );
    let mint = StateWithExtensions::<Mint>::unpack(&after.data).unwrap();
    let metadata = mint.get_variable_len_extension::<TokenMetadata>().unwrap();
    assert_eq!(metadata.mint, f.mint.pubkey());
    assert_eq!(
        Option::<Pubkey>::from(metadata.update_authority),
        Some(f.authority.pubkey())
    );
    assert_eq!(metadata.name, name);
    assert_eq!(metadata.symbol, symbol);
    assert_eq!(metadata.uri, uri);
    assert!(metadata.additional_metadata.is_empty());
}

#[test]
fn uses_existing_rent_funding_without_adding_more_lamports() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    f.svm.airdrop(&f.mint.pubkey(), 10_000_000).unwrap();
    let before = f.svm.get_account(&f.mint.pubkey()).unwrap();
    let ix = metadata_instruction(&f, "Remittance", "RUSD", "https://example.com/token.json");
    send(&mut f.svm, &f.payer, &[ix], &[&f.authority]).unwrap();
    let after = f.svm.get_account(&f.mint.pubkey()).unwrap();
    assert_eq!(after.lamports, before.lamports);
    assert!(after.data.len() > before.data.len());
    assert!(after.lamports >= f.svm.minimum_balance_for_rent_exemption(after.data.len()));
}

#[test]
fn rejects_a_signer_who_is_not_the_mint_authority() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    let before = f.svm.get_account(&f.mint.pubkey()).unwrap();
    let mut ix = metadata_instruction(&f, "Remittance", "RUSD", "");
    ix.accounts
        .iter_mut()
        .find(|account| account.pubkey == f.authority.pubkey())
        .unwrap()
        .pubkey = f.payer.pubkey();
    let error = send(&mut f.svm, &f.payer, &[ix], &[]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("ConstraintMintMintAuthority")),
        "{error:?}"
    );
    let after = f.svm.get_account(&f.mint.pubkey()).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
}

#[test]
fn requires_the_mint_authority_signature() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    let before = f.svm.get_account(&f.mint.pubkey()).unwrap();
    let mut ix = metadata_instruction(&f, "Remittance", "RUSD", "");
    ix.accounts
        .iter_mut()
        .find(|account| account.pubkey == f.authority.pubkey())
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
    let after = f.svm.get_account(&f.mint.pubkey()).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
}

#[test]
fn rejects_a_pointer_to_another_account() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    let update = metadata_pointer::instruction::update(
        &spl_token_2022::id(),
        &f.mint.pubkey(),
        &f.authority.pubkey(),
        &[],
        Some(Keypair::new().pubkey()),
    )
    .unwrap();
    send(&mut f.svm, &f.payer, &[update], &[&f.authority]).unwrap();
    let before = f.svm.get_account(&f.mint.pubkey()).unwrap();
    let ix = metadata_instruction(&f, "Remittance", "RUSD", "");
    let error = send(&mut f.svm, &f.payer, &[ix], &[&f.authority]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("ConstraintMintMetadataPointerExtensionMetadataAddress")),
        "{error:?}"
    );
    let after = f.svm.get_account(&f.mint.pubkey()).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
}

#[test]
fn rejects_duplicate_initialization_without_replacing_metadata_or_funding_rent() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    let ix = metadata_instruction(&f, "Remittance", "RUSD", "");
    send(&mut f.svm, &f.payer, &[ix], &[&f.authority]).unwrap();
    let before = f.svm.get_account(&f.mint.pubkey()).unwrap();
    let ix = metadata_instruction(&f, "Replacement name", "NEW", &"x".repeat(300));
    let error = send(&mut f.svm, &f.payer, &[ix], &[&f.authority]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("MetadataAlreadyInitialized")),
        "{error:?}"
    );
    let after = f.svm.get_account(&f.mint.pubkey()).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
}

#[test]
fn rejects_an_account_not_owned_by_token_2022() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    let before = f.svm.get_account(&f.authority.pubkey()).unwrap();
    let mut ix = metadata_instruction(&f, "Remittance", "RUSD", "");
    ix.accounts
        .iter_mut()
        .find(|account| account.pubkey == f.mint.pubkey())
        .unwrap()
        .pubkey = f.authority.pubkey();
    let error = send(&mut f.svm, &f.payer, &[ix], &[&f.authority]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("AccountOwnedByWrongProgram")),
        "{error:?}"
    );
    let after = f.svm.get_account(&f.authority.pubkey()).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
}

#[test]
fn insufficient_rent_funding_leaves_the_mint_and_funding_account_unchanged() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    let funding = Keypair::new();
    f.svm.airdrop(&funding.pubkey(), 1_000_000).unwrap();
    let before = f.svm.get_account(&f.mint.pubkey()).unwrap();
    let mut ix = metadata_instruction(&f, "Remittance", "RUSD", &"x".repeat(500));
    ix.accounts
        .iter_mut()
        .find(|account| account.pubkey == f.payer.pubkey())
        .unwrap()
        .pubkey = funding.pubkey();
    // Keep the transaction fee payer separate from the account funding mint rent.
    let error = send(&mut f.svm, &f.payer, &[ix], &[&funding, &f.authority]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("insufficient lamports")),
        "{error:?}"
    );
    let after = f.svm.get_account(&f.mint.pubkey()).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
    assert_eq!(
        f.svm.get_account(&funding.pubkey()).unwrap().lamports,
        1_000_000
    );
}
