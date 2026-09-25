mod common;

use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, program_option::COption, system_program},
        InstructionData, ToAccountMetas,
    },
    anchor_spl::{
        associated_token::spl_associated_token_account,
        token_2022::spl_token_2022::{
            self,
            extension::{
                confidential_transfer::ConfidentialTransferMint,
                confidential_transfer_fee::ConfidentialTransferFeeConfig,
                default_account_state::DefaultAccountState,
                metadata_pointer::MetadataPointer,
                mint_close_authority::MintCloseAuthority,
                permanent_delegate::PermanentDelegate,
                transfer_fee::{self, TransferFeeAmount, TransferFeeConfig},
                BaseStateWithExtensions, ExtensionType, StateWithExtensions,
            },
            solana_zk_sdk::encryption::elgamal::ElGamalKeypair,
            state::{Account, AccountState, Mint},
        },
    },
    common::{send, Fixture},
    solana_keypair::Keypair,
    solana_signer::Signer,
    spl_token_metadata_interface::state::TokenMetadata,
};

fn initialize_instruction(f: &Fixture, fee_key: [u8; 32], basis_points: u16) -> Instruction {
    Instruction::new_with_bytes(
        remittance_stablecoin::id(),
        &remittance_stablecoin::instruction::InitializeConfidentialMint {
            decimals: 6,
            transfer_fee_basis_points: basis_points,
            maximum_fee: 50_000,
            withdraw_withheld_authority_elgamal_pubkey: fee_key,
        }
        .data(),
        remittance_stablecoin::accounts::InitializeConfidentialMint {
            payer: f.payer.pubkey(),
            mint: f.mint.pubkey(),
            authority: f.authority.pubkey(),
            token_program: spl_token_2022::id(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn initialize(f: &mut Fixture) -> ElGamalKeypair {
    let fee_key = ElGamalKeypair::new_rand();
    let ix = initialize_instruction(f, fee_key.pubkey().into(), 100);
    send(&mut f.svm, &f.payer, &[ix], &[&f.mint, &f.authority]).unwrap();
    fee_key
}

fn create_account(f: &mut Fixture, owner: Pubkey) -> Pubkey {
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

#[test]
fn initializes_all_seven_extensions_with_manual_approval_and_issuer_authorities() {
    let mut f = Fixture::new();
    let fee_key = initialize(&mut f);
    let account = f.svm.get_account(&f.mint.pubkey()).unwrap();
    let extensions = [
        ExtensionType::TransferFeeConfig,
        ExtensionType::MetadataPointer,
        ExtensionType::DefaultAccountState,
        ExtensionType::MintCloseAuthority,
        ExtensionType::PermanentDelegate,
        ExtensionType::ConfidentialTransferMint,
        ExtensionType::ConfidentialTransferFeeConfig,
    ];
    let space = ExtensionType::try_calculate_account_len::<Mint>(&extensions).unwrap();
    assert_eq!(account.owner, spl_token_2022::id());
    assert_eq!(account.data.len(), space);
    assert_eq!(
        account.lamports,
        f.svm.minimum_balance_for_rent_exemption(space)
    );
    let mint = StateWithExtensions::<Mint>::unpack(&account.data).unwrap();
    assert!(mint.base.is_initialized);
    assert_eq!(mint.base.decimals, 6);
    assert_eq!(mint.base.supply, 0);
    assert_eq!(
        mint.base.mint_authority,
        COption::Some(f.authority.pubkey())
    );
    assert_eq!(
        mint.base.freeze_authority,
        COption::Some(f.authority.pubkey())
    );
    assert_eq!(mint.get_extension_types().unwrap(), extensions);
    let fee = mint.get_extension::<TransferFeeConfig>().unwrap();
    assert_eq!(
        Option::<Pubkey>::from(fee.transfer_fee_config_authority),
        Some(f.authority.pubkey())
    );
    assert_eq!(
        Option::<Pubkey>::from(fee.withdraw_withheld_authority),
        Some(f.authority.pubkey())
    );
    assert_eq!(u64::from(fee.withheld_amount), 0);
    for schedule in [fee.older_transfer_fee, fee.newer_transfer_fee] {
        assert_eq!(u16::from(schedule.transfer_fee_basis_points), 100);
        assert_eq!(u64::from(schedule.maximum_fee), 50_000);
    }
    let pointer = mint.get_extension::<MetadataPointer>().unwrap();
    assert_eq!(
        Option::<Pubkey>::from(pointer.authority),
        Some(f.authority.pubkey())
    );
    assert_eq!(
        Option::<Pubkey>::from(pointer.metadata_address),
        Some(f.mint.pubkey())
    );
    assert_eq!(
        mint.get_extension::<DefaultAccountState>().unwrap().state,
        AccountState::Frozen as u8
    );
    assert_eq!(
        Option::<Pubkey>::from(
            mint.get_extension::<MintCloseAuthority>()
                .unwrap()
                .close_authority
        ),
        Some(f.authority.pubkey())
    );
    assert_eq!(
        Option::<Pubkey>::from(mint.get_extension::<PermanentDelegate>().unwrap().delegate),
        Some(f.authority.pubkey())
    );
    let confidential = mint.get_extension::<ConfidentialTransferMint>().unwrap();
    assert_eq!(
        Option::<Pubkey>::from(confidential.authority),
        Some(f.authority.pubkey())
    );
    assert!(!bool::from(confidential.auto_approve_new_accounts));
    assert_eq!(confidential.auditor_elgamal_pubkey, Default::default());
    let confidential_fee = mint
        .get_extension::<ConfidentialTransferFeeConfig>()
        .unwrap();
    assert_eq!(
        Option::<Pubkey>::from(confidential_fee.authority),
        Some(f.authority.pubkey())
    );
    assert_eq!(
        confidential_fee.withdraw_withheld_authority_elgamal_pubkey,
        (*fee_key.pubkey()).into()
    );
    assert!(bool::from(confidential_fee.harvest_to_mint_enabled));
    assert_eq!(confidential_fee.withheld_amount, Default::default());
}

#[test]
fn supports_existing_metadata_initialization_and_frozen_token_accounts() {
    let mut f = Fixture::new();
    initialize(&mut f);
    let before = f.svm.get_account(&f.mint.pubkey()).unwrap();
    let ix = Instruction::new_with_bytes(
        remittance_stablecoin::id(),
        &remittance_stablecoin::instruction::InitializeMetadata {
            name: "Confidential Remittance".into(),
            symbol: "CRUSD".into(),
            uri: "https://example.com/token.json".into(),
        }
        .data(),
        remittance_stablecoin::accounts::InitializeMetadata {
            payer: f.payer.pubkey(),
            mint: f.mint.pubkey(),
            authority: f.authority.pubkey(),
            token_program: spl_token_2022::id(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );
    send(&mut f.svm, &f.payer, &[ix], &[&f.authority]).unwrap();
    let after = f.svm.get_account(&f.mint.pubkey()).unwrap();
    assert_eq!(&after.data[..before.data.len()], &before.data);
    let mint = StateWithExtensions::<Mint>::unpack(&after.data).unwrap();
    let metadata = mint.get_variable_len_extension::<TokenMetadata>().unwrap();
    assert_eq!(metadata.name, "Confidential Remittance");
    assert_eq!(metadata.mint, f.mint.pubkey());
    let ata = create_account(&mut f, Keypair::new().pubkey());
    let account = f.svm.get_account(&ata).unwrap();
    let state = StateWithExtensions::<Account>::unpack(&account.data).unwrap();
    assert_eq!(state.base.state, AccountState::Frozen);
    assert_eq!(state.base.mint, f.mint.pubkey());
}

#[test]
fn permanent_delegate_can_transfer_public_funds_with_fees_without_the_owner() {
    let mut f = Fixture::new();
    initialize(&mut f);
    let source = create_account(&mut f, Keypair::new().pubkey());
    let destination = create_account(&mut f, Keypair::new().pubkey());
    for token_account in [source, destination] {
        let ix = Instruction::new_with_bytes(
            remittance_stablecoin::id(),
            &remittance_stablecoin::instruction::ThawAccount {}.data(),
            remittance_stablecoin::accounts::ThawAccount {
                mint: f.mint.pubkey(),
                token_account,
                authority: f.authority.pubkey(),
                token_program: spl_token_2022::id(),
            }
            .to_account_metas(None),
        );
        send(&mut f.svm, &f.payer, &[ix], &[&f.authority]).unwrap();
    }
    let mint_to = spl_token_2022::instruction::mint_to_checked(
        &spl_token_2022::id(),
        &f.mint.pubkey(),
        &source,
        &f.authority.pubkey(),
        &[],
        1_000_000,
        6,
    )
    .unwrap();
    send(&mut f.svm, &f.payer, &[mint_to], &[&f.authority]).unwrap();
    // Exercise the configured permanent delegate through Token-2022 directly.
    let transfer = transfer_fee::instruction::transfer_checked_with_fee(
        &spl_token_2022::id(),
        &source,
        &f.mint.pubkey(),
        &destination,
        &f.authority.pubkey(),
        &[],
        1_000_000,
        6,
        10_000,
    )
    .unwrap();
    send(&mut f.svm, &f.payer, &[transfer], &[&f.authority]).unwrap();
    let source = f.svm.get_account(&source).unwrap();
    let destination = f.svm.get_account(&destination).unwrap();
    let source = StateWithExtensions::<Account>::unpack(&source.data).unwrap();
    let destination = StateWithExtensions::<Account>::unpack(&destination.data).unwrap();
    assert_eq!(source.base.amount, 0);
    assert_eq!(destination.base.amount, 990_000);
    assert_eq!(
        u64::from(
            destination
                .get_extension::<TransferFeeAmount>()
                .unwrap()
                .withheld_amount
        ),
        10_000
    );
}

#[test]
fn reissuing_creates_a_separate_mint_without_changing_the_original() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    let old_mint = f.mint.pubkey();
    let before = f.svm.get_account(&old_mint).unwrap();
    f.mint = Keypair::new();
    initialize(&mut f);
    let after = f.svm.get_account(&old_mint).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
    assert_eq!(after.owner, before.owner);
    let original = StateWithExtensions::<Mint>::unpack(&after.data).unwrap();
    assert!(!original
        .get_extension_types()
        .unwrap()
        .contains(&ExtensionType::ConfidentialTransferMint));
}

#[test]
fn rejects_reinitialization_without_changing_the_mint() {
    let mut f = Fixture::new();
    let fee_key = initialize(&mut f);
    let before = f.svm.get_account(&f.mint.pubkey()).unwrap();
    f.svm.expire_blockhash();
    let ix = initialize_instruction(&f, fee_key.pubkey().into(), 100);
    let error = send(&mut f.svm, &f.payer, &[ix], &[&f.mint, &f.authority]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("already in use")),
        "{error:?}"
    );
    let after = f.svm.get_account(&f.mint.pubkey()).unwrap();
    assert_eq!(after.data, before.data);
    assert_eq!(after.lamports, before.lamports);
}

#[test]
fn rejects_excessive_fees_without_creating_a_mint() {
    let mut f = Fixture::new();
    let fee_key = ElGamalKeypair::new_rand();
    let ix = initialize_instruction(&f, fee_key.pubkey().into(), 10_001);
    let error = send(&mut f.svm, &f.payer, &[ix], &[&f.mint, &f.authority]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("Transfer fee exceeds maximum")),
        "{error:?}"
    );
    assert!(f.svm.get_account(&f.mint.pubkey()).is_none());
}

#[test]
fn requires_both_the_issuer_and_new_mint_signatures() {
    for omit_issuer in [true, false] {
        let mut f = Fixture::new();
        let fee_key = ElGamalKeypair::new_rand();
        let mut ix = initialize_instruction(&f, fee_key.pubkey().into(), 100);
        let missing = if omit_issuer {
            f.authority.pubkey()
        } else {
            f.mint.pubkey()
        };
        ix.accounts
            .iter_mut()
            .find(|a| a.pubkey == missing)
            .unwrap()
            .is_signer = false;
        let signer = if omit_issuer { &f.mint } else { &f.authority };
        let error = send(&mut f.svm, &f.payer, &[ix], &[signer]).unwrap_err();
        assert!(
            error
                .meta
                .logs
                .iter()
                .any(|log| log.contains("AccountNotSigner")),
            "{error:?}"
        );
        assert!(f.svm.get_account(&f.mint.pubkey()).is_none());
    }
}

#[test]
fn rejects_the_legacy_token_program_before_creating_a_mint() {
    let mut f = Fixture::new();
    let fee_key = ElGamalKeypair::new_rand();
    let mut ix = initialize_instruction(&f, fee_key.pubkey().into(), 100);
    ix.accounts
        .iter_mut()
        .find(|a| a.pubkey == spl_token_2022::id())
        .unwrap()
        .pubkey = anchor_spl::token::ID;
    let error = send(&mut f.svm, &f.payer, &[ix], &[&f.mint, &f.authority]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("InvalidProgramId")),
        "{error:?}"
    );
    assert!(f.svm.get_account(&f.mint.pubkey()).is_none());
}
