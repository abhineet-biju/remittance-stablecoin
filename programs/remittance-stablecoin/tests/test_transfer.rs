mod common;

use {
    anchor_lang::{
        prelude::{Clock, Pubkey},
        solana_program::instruction::Instruction,
        InstructionData, ToAccountMetas,
    },
    anchor_spl::{
        associated_token::spl_associated_token_account,
        token_2022::spl_token_2022::{
            self,
            extension::{
                transfer_fee::{self, TransferFeeAmount, TransferFeeConfig},
                BaseStateWithExtensions, StateWithExtensions,
            },
            state::{Account, Mint},
        },
    },
    common::{send, Fixture},
    litesvm::types::TransactionResult,
    solana_keypair::Keypair,
    solana_signer::Signer,
};

struct TransferFixture {
    mint: Fixture,
    owner: Keypair,
    source: Pubkey,
    destination: Pubkey,
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

impl TransferFixture {
    fn new(basis_points: u16) -> Self {
        let mut f = Fixture::new();
        f.initialize(basis_points).unwrap();
        let owner = Keypair::new();
        f.svm.airdrop(&owner.pubkey(), 1_000_000).unwrap();
        let source = create_account(&mut f, owner.pubkey());
        let destination = create_account(&mut f, Keypair::new().pubkey());
        let instructions = [
            thaw_instruction(&f, source),
            thaw_instruction(&f, destination),
        ];
        send(&mut f.svm, &f.payer, &instructions, &[&f.authority]).unwrap();
        let mint_to = spl_token_2022::instruction::mint_to_checked(
            &spl_token_2022::id(),
            &f.mint.pubkey(),
            &source,
            &f.authority.pubkey(),
            &[],
            20_000_000,
            6,
        )
        .unwrap();
        send(&mut f.svm, &f.payer, &[mint_to], &[&f.authority]).unwrap();
        Self {
            mint: f,
            owner,
            source,
            destination,
        }
    }

    fn instruction(&self, amount: u64) -> Instruction {
        Instruction::new_with_bytes(
            remittance_stablecoin::id(),
            &remittance_stablecoin::instruction::Transfer { amount }.data(),
            remittance_stablecoin::accounts::Transfer {
                owner: self.owner.pubkey(),
                mint: self.mint.mint.pubkey(),
                source: self.source,
                destination: self.destination,
                token_program: spl_token_2022::id(),
            }
            .to_account_metas(None),
        )
    }

    fn transfer(&mut self, amount: u64) -> TransactionResult {
        let ix = self.instruction(amount);
        send(&mut self.mint.svm, &self.mint.payer, &[ix], &[&self.owner])
    }

    fn balances(&self) -> (u64, u64, u64) {
        let source = self.mint.svm.get_account(&self.source).unwrap();
        let destination = self.mint.svm.get_account(&self.destination).unwrap();
        let source = StateWithExtensions::<Account>::unpack(&source.data).unwrap();
        let destination = StateWithExtensions::<Account>::unpack(&destination.data).unwrap();
        (
            source.base.amount,
            destination.base.amount,
            u64::from(
                destination
                    .get_extension::<TransferFeeAmount>()
                    .unwrap()
                    .withheld_amount,
            ),
        )
    }

    fn snapshot(&self) -> Vec<Vec<u8>> {
        [self.source, self.destination, self.mint.mint.pubkey()]
            .iter()
            .map(|key| self.mint.svm.get_account(key).unwrap().data)
            .collect()
    }
}

#[test]
fn debits_the_gross_amount_and_withholds_the_fee_on_the_recipient() {
    let mut f = TransferFixture::new(100);
    let mint_before = f.mint.svm.get_account(&f.mint.mint.pubkey()).unwrap().data;
    let result = f.transfer(1_000_000).unwrap();
    assert!(
        result
            .logs
            .iter()
            .any(|log| log.contains("TransferCheckedWithFee")),
        "{result:?}"
    );
    assert_eq!(f.balances(), (19_000_000, 990_000, 10_000));
    assert_eq!(
        f.mint.svm.get_account(&f.mint.mint.pubkey()).unwrap().data,
        mint_before
    );
    let (source, destination, withheld) = f.balances();
    assert_eq!(source + destination + withheld, 20_000_000);
}

#[test]
fn rounds_fees_up_and_caps_them_at_the_configured_maximum() {
    let mut f = TransferFixture::new(100);
    f.transfer(101).unwrap();
    assert_eq!(f.balances(), (19_999_899, 99, 2));
    f.transfer(10_000_000).unwrap();
    assert_eq!(f.balances(), (9_999_899, 9_950_099, 50_002));
}

#[test]
fn applies_a_scheduled_fee_only_when_its_epoch_becomes_active() {
    let mut f = TransferFixture::new(100);
    let update = transfer_fee::instruction::set_transfer_fee(
        &spl_token_2022::id(),
        &f.mint.mint.pubkey(),
        &f.mint.authority.pubkey(),
        &[],
        250,
        50_000,
    )
    .unwrap();
    send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[update],
        &[&f.mint.authority],
    )
    .unwrap();
    let account = f.mint.svm.get_account(&f.mint.mint.pubkey()).unwrap();
    let mint = StateWithExtensions::<Mint>::unpack(&account.data).unwrap();
    let config = mint.get_extension::<TransferFeeConfig>().unwrap();
    let activation_epoch = u64::from(config.newer_transfer_fee.epoch);
    let mut clock = f.mint.svm.get_sysvar::<Clock>();
    assert!(activation_epoch > clock.epoch);
    clock.epoch = activation_epoch - 1;
    f.mint.svm.set_sysvar(&clock);
    f.transfer(1_000_000).unwrap();
    assert_eq!(f.balances(), (19_000_000, 990_000, 10_000));
    clock.epoch = activation_epoch;
    f.mint.svm.set_sysvar(&clock);
    f.mint.svm.expire_blockhash();
    f.transfer(1_000_000).unwrap();
    assert_eq!(f.balances(), (18_000_000, 1_965_000, 35_000));
}

#[test]
fn handles_zero_amount_zero_fee_and_full_balance_transfers() {
    let mut f = TransferFixture::new(0);
    let before = f.snapshot();
    f.transfer(0).unwrap();
    assert_eq!(f.snapshot(), before);
    f.transfer(20_000_000).unwrap();
    assert_eq!(f.balances(), (0, 20_000_000, 0));
}

#[test]
fn rejects_insufficient_funds_without_mutating_balances_or_fees() {
    let mut f = TransferFixture::new(100);
    let before = f.snapshot();
    let error = f.transfer(20_000_001).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("insufficient funds")),
        "{error:?}"
    );
    assert_eq!(f.snapshot(), before);
}

#[test]
fn rejects_frozen_source_and_destination_accounts_without_mutation() {
    for freeze_source in [true, false] {
        let mut f = TransferFixture::new(100);
        let freeze = spl_token_2022::instruction::freeze_account(
            &spl_token_2022::id(),
            &if freeze_source {
                f.source
            } else {
                f.destination
            },
            &f.mint.mint.pubkey(),
            &f.mint.authority.pubkey(),
            &[],
        )
        .unwrap();
        send(
            &mut f.mint.svm,
            &f.mint.payer,
            &[freeze],
            &[&f.mint.authority],
        )
        .unwrap();
        let before = f.snapshot();
        let error = f.transfer(1_000_000).unwrap_err();
        assert!(
            error
                .meta
                .logs
                .iter()
                .any(|log| log.contains("Account is frozen")),
            "{error:?}"
        );
        assert_eq!(f.snapshot(), before);
    }
}

#[test]
fn rejects_a_signer_who_does_not_own_the_source() {
    let mut f = TransferFixture::new(100);
    let before = f.snapshot();
    let mut ix = f.instruction(1_000_000);
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
    assert_eq!(f.snapshot(), before);
}

#[test]
fn requires_the_source_owner_signature() {
    let mut f = TransferFixture::new(100);
    let before = f.snapshot();
    let mut ix = f.instruction(1_000_000);
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
    assert_eq!(f.snapshot(), before);
}

#[test]
fn rejects_source_and_destination_accounts_from_another_mint() {
    for replace_source in [true, false] {
        let mut f = TransferFixture::new(100);
        let original_mint = std::mem::replace(&mut f.mint.mint, Keypair::new());
        f.mint.initialize(100).unwrap();
        let foreign = create_account(&mut f.mint, f.owner.pubkey());
        f.mint.mint = original_mint;
        let before = f.snapshot();
        let foreign_before = f.mint.svm.get_account(&foreign).unwrap().data;
        let mut ix = f.instruction(1_000_000);
        let replaced = if replace_source {
            f.source
        } else {
            f.destination
        };
        ix.accounts
            .iter_mut()
            .find(|a| a.pubkey == replaced)
            .unwrap()
            .pubkey = foreign;
        let error = send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.owner]).unwrap_err();
        assert!(
            error
                .meta
                .logs
                .iter()
                .any(|log| log.contains("ConstraintTokenMint")),
            "{error:?}"
        );
        assert_eq!(f.snapshot(), before);
        assert_eq!(
            f.mint.svm.get_account(&foreign).unwrap().data,
            foreign_before
        );
    }
}

#[test]
fn rejects_the_legacy_token_program_without_mutation() {
    let mut f = TransferFixture::new(100);
    let before = f.snapshot();
    let mut ix = f.instruction(1_000_000);
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
    assert_eq!(f.snapshot(), before);
}

#[test]
fn rejects_self_transfers_without_mutating_balances_or_fees() {
    let mut f = TransferFixture::new(100);
    let before = f.snapshot();
    let mut ix = f.instruction(1_000_000);
    ix.accounts
        .iter_mut()
        .find(|a| a.pubkey == f.destination)
        .unwrap()
        .pubkey = f.source;
    let error = send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.owner]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|log| log.contains("ConstraintDuplicateMutableAccount")),
        "{error:?}"
    );
    assert_eq!(f.snapshot(), before);
}
