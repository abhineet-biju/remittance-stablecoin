mod common;
#[path = "common/confidential.rs"]
mod confidential;
#[path = "common/lifecycle.rs"]
mod lifecycle;
use {
    anchor_lang::InstructionData, anchor_spl::token_2022::spl_token_2022, common::send,
    confidential::ConfigureFixture, lifecycle::*, solana_signer::Signer,
};

#[test]
fn deposits_into_pending_without_changing_supply_or_available_balance() {
    let mut f = setup(5_000_000);
    let mint_before = f.mint.svm.get_account(&f.mint.mint.pubkey()).unwrap().data;
    deposit(&mut f, 1_000_001);
    assert_balances(&f, 3_999_999, 1_000_001, 0);
    deposit(&mut f, 2_000_002);
    assert_balances(&f, 1_999_997, 3_000_003, 0);
    assert_eq!(
        u64::from(extension(&f.mint, f.ata).pending_balance_credit_counter),
        2
    );
    assert_eq!(
        f.mint.svm.get_account(&f.mint.mint.pubkey()).unwrap().data,
        mint_before
    );
}
#[test]
fn rejects_unapproved_and_frozen_accounts() {
    let mut f = ConfigureFixture::new();
    f.configure().unwrap();
    f.thaw_and_mint(100);
    let before = f.mint.svm.get_account(&f.ata).unwrap().data;
    let ix = deposit_instruction(&f, 10);
    let error = send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.owner]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|l| l.to_lowercase().contains("not approved")),
        "{error:?}"
    );
    assert_eq!(f.mint.svm.get_account(&f.ata).unwrap().data, before);
    approve(&mut f);
    let freeze = spl_token_2022::instruction::freeze_account(
        &spl_token_2022::id(),
        &f.ata,
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
    let before = f.mint.svm.get_account(&f.ata).unwrap().data;
    let ix = deposit_instruction(&f, 11);
    let error = send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.owner]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|l| l.to_lowercase().contains("frozen")),
        "{error:?}"
    );
    assert_eq!(f.mint.svm.get_account(&f.ata).unwrap().data, before);
}
#[test]
fn rejects_insufficient_funds_and_amounts_at_the_48_bit_limit() {
    let mut f = setup(1 << 48);
    for amount in [(1 << 48) + 1, 1 << 48] {
        let before = f.mint.svm.get_account(&f.ata).unwrap().data;
        let ix = deposit_instruction(&f, amount);
        assert!(send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.owner]).is_err());
        assert_eq!(f.mint.svm.get_account(&f.ata).unwrap().data, before);
    }
    let mut f = setup(10);
    let ix = deposit_instruction(&f, 11);
    let error = send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.owner]).unwrap_err();
    assert!(
        error
            .meta
            .logs
            .iter()
            .any(|l| l.contains("Operation overflowed")),
        "{error:?}"
    );
    assert_balances(&f, 10, 0, 0);
}
#[test]
fn rejects_wrong_owner_and_missing_signature() {
    let mut f = setup(100);
    for missing in [true, false] {
        let mut ix = deposit_instruction(&f, 10);
        let owner = ix
            .accounts
            .iter_mut()
            .find(|a| a.pubkey == f.owner.pubkey())
            .unwrap();
        if missing {
            owner.is_signer = false;
        } else {
            owner.pubkey = f.mint.payer.pubkey();
        }
        let error = send(&mut f.mint.svm, &f.mint.payer, &[ix], &[]).unwrap_err();
        assert!(
            error.meta.logs.iter().any(|l| l.contains(if missing {
                "AccountNotSigner"
            } else {
                "ConstraintTokenOwner"
            })),
            "{error:?}"
        );
        assert_balances(&f, 100, 0, 0);
    }
}
#[test]
fn enforces_the_pending_credit_limit() {
    let mut f = ConfigureFixture::new();
    let mut ix = f.instruction();
    ix.data = remittance_stablecoin::instruction::ConfigureAccount {
        decryptable_zero_balance: f.aes.encrypt(0).to_bytes(),
        maximum_pending_balance_credit_counter: 1,
    }
    .data();
    send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.owner]).unwrap();
    approve(&mut f);
    f.thaw_and_mint(100);
    deposit(&mut f, 10);
    let before = f.mint.svm.get_account(&f.ata).unwrap().data;
    let ix = deposit_instruction(&f, 20);
    assert!(send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.owner]).is_err());
    assert_eq!(f.mint.svm.get_account(&f.ata).unwrap().data, before);
}
