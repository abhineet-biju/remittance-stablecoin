mod common;

use {
    anchor_lang::{prelude::Pubkey, solana_program::program_option::COption},
    anchor_spl::{
        associated_token::spl_associated_token_account,
        token_2022::spl_token_2022::{
            self,
            extension::{
                default_account_state::DefaultAccountState, metadata_pointer::MetadataPointer,
                mint_close_authority::MintCloseAuthority, transfer_fee::TransferFeeConfig,
                BaseStateWithExtensions, ExtensionType, StateWithExtensions,
            },
            state::{Account, AccountState, Mint},
        },
    },
    common::{send, Fixture},
    solana_keypair::Keypair,
    solana_signer::Signer,
};

#[test]
fn initializes_the_mint_with_all_four_extensions_and_issuer_authorities() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();

    let account = f.svm.get_account(&f.mint.pubkey()).unwrap();
    assert_eq!(account.owner, spl_token_2022::id());
    let extensions = [
        ExtensionType::TransferFeeConfig,
        ExtensionType::MetadataPointer,
        ExtensionType::DefaultAccountState,
        ExtensionType::MintCloseAuthority,
    ];
    let space = ExtensionType::try_calculate_account_len::<Mint>(&extensions).unwrap();
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
    let default_state = mint.get_extension::<DefaultAccountState>().unwrap();
    assert_eq!(default_state.state, AccountState::Frozen as u8);
    let close = mint.get_extension::<MintCloseAuthority>().unwrap();
    assert_eq!(
        Option::<Pubkey>::from(close.close_authority),
        Some(f.authority.pubkey())
    );
}

#[test]
fn new_token_accounts_inherit_the_frozen_default() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    let owner = Keypair::new().pubkey();
    let ata = spl_associated_token_account::address::get_associated_token_address_with_program_id(
        &owner,
        &f.mint.pubkey(),
        &spl_token_2022::id(),
    );
    let create = spl_associated_token_account::instruction::create_associated_token_account(
        &f.payer.pubkey(),
        &owner,
        &f.mint.pubkey(),
        &spl_token_2022::id(),
    );
    send(&mut f.svm, &f.payer, &[create], &[]).unwrap();
    let account = f.svm.get_account(&ata).unwrap();
    let state = StateWithExtensions::<Account>::unpack(&account.data).unwrap();
    assert_eq!(state.base.state, AccountState::Frozen);
    assert_eq!(state.base.owner, owner);
    assert_eq!(state.base.mint, f.mint.pubkey());
}

#[test]
fn rejects_fees_above_one_hundred_percent_without_creating_a_mint() {
    let mut f = Fixture::new();
    let error = f.initialize(10_001).unwrap_err();
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
fn requires_the_issuer_to_sign() {
    let mut f = Fixture::new();
    let mut ix = f.instruction(100);
    ix.accounts
        .iter_mut()
        .find(|account| account.pubkey == f.authority.pubkey())
        .unwrap()
        .is_signer = false;
    let error = send(&mut f.svm, &f.payer, &[ix], &[&f.mint]).unwrap_err();
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

#[test]
fn rejects_reinitialization_without_changing_the_mint() {
    let mut f = Fixture::new();
    f.initialize(100).unwrap();
    let before = f.svm.get_account(&f.mint.pubkey()).unwrap();
    // Use a fresh blockhash so the same instruction reaches the program again.
    f.svm.expire_blockhash();
    let error = f.initialize(100).unwrap_err();
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
    assert_eq!(after.owner, before.owner);
}

#[test]
fn rejects_the_legacy_token_program_before_creating_the_mint() {
    let mut f = Fixture::new();
    let mut ix = f.instruction(100);
    ix.accounts
        .iter_mut()
        .find(|account| account.pubkey == spl_token_2022::id())
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

#[test]
fn requires_the_new_mint_to_sign() {
    let mut f = Fixture::new();
    let mut ix = f.instruction(100);
    ix.accounts
        .iter_mut()
        .find(|account| account.pubkey == f.mint.pubkey())
        .unwrap()
        .is_signer = false;
    let error = send(&mut f.svm, &f.payer, &[ix], &[&f.authority]).unwrap_err();
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

#[test]
fn accepts_zero_and_one_hundred_percent_fees_with_a_cap() {
    for basis_points in [0, 10_000] {
        let mut f = Fixture::new();
        f.initialize(basis_points).unwrap();
        let account = f.svm.get_account(&f.mint.pubkey()).unwrap();
        let mint = StateWithExtensions::<Mint>::unpack(&account.data).unwrap();
        let fee = mint.get_extension::<TransferFeeConfig>().unwrap();
        for schedule in [fee.older_transfer_fee, fee.newer_transfer_fee] {
            assert_eq!(u16::from(schedule.transfer_fee_basis_points), basis_points);
            assert_eq!(u64::from(schedule.maximum_fee), 50_000);
        }
    }
}
