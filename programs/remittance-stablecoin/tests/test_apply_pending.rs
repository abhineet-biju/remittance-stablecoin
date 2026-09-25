mod common;
#[path = "common/confidential.rs"]
mod confidential;
#[path = "common/lifecycle.rs"]
mod lifecycle;
use {
    common::send,
    lifecycle::*,
    solana_signer::Signer,
    solana_zk_sdk::encryption::{auth_encryption::AeCiphertext, elgamal::ElGamalCiphertext},
};

#[test]
fn consolidates_pending_balances_and_resets_the_credit_counter() {
    let mut f = setup(5_000_000);
    deposit(&mut f, 1_000_001);
    deposit(&mut f, 2_000_002);
    apply(&mut f, 3_000_003);
    assert_balances(&f, 1_999_997, 0, 3_000_003);
    let e = extension(&f.mint, f.ata);
    assert_eq!(u64::from(e.pending_balance_credit_counter), 0);
    assert_eq!(u64::from(e.expected_pending_balance_credit_counter), 2);
    assert_eq!(u64::from(e.actual_pending_balance_credit_counter), 2);
    deposit(&mut f, 7);
    apply(&mut f, 3_000_010);
    assert_balances(&f, 1_999_990, 0, 3_000_010);
}
#[test]
fn applying_empty_pending_keeps_funds_unchanged() {
    let mut f = setup(100);
    apply(&mut f, 0);
    assert_balances(&f, 100, 0, 0);
    deposit(&mut f, 50);
    apply(&mut f, 50);
    apply(&mut f, 50);
    assert_balances(&f, 50, 0, 50);
}
#[test]
fn records_a_credit_race_so_the_client_can_reconcile_its_encrypted_balance() {
    let mut f = setup(100);
    deposit(&mut f, 10);
    let ix = apply_instruction(&f.mint, f.owner.pubkey(), f.ata, &f.aes, 10);
    deposit(&mut f, 20);
    send(&mut f.mint.svm, &f.mint.payer, &[ix], &[&f.owner]).unwrap();
    let e = extension(&f.mint, f.ata);
    assert_eq!(u64::from(e.actual_pending_balance_credit_counter), 2);
    assert_eq!(u64::from(e.expected_pending_balance_credit_counter), 1);
    assert_eq!(
        ElGamalCiphertext::from_bytes(bytemuck::bytes_of(&e.available_balance))
            .unwrap()
            .decrypt_u32(f.elgamal.secret()),
        Some(30)
    );
    assert_eq!(
        AeCiphertext::from_bytes(bytemuck::bytes_of(&e.decryptable_available_balance))
            .unwrap()
            .decrypt(&f.aes),
        Some(10)
    );
    // The owner reconciles the actual encrypted balance before creating spending proofs.
    apply(&mut f, 30);
    assert_balances(&f, 70, 0, 30);
}
#[test]
fn rejects_missing_signature_and_wrong_owner_without_applying_funds() {
    let mut f = setup(100);
    deposit(&mut f, 50);
    let before = f.mint.svm.get_account(&f.ata).unwrap().data;
    for missing in [true, false] {
        let mut ix = apply_instruction(&f.mint, f.owner.pubkey(), f.ata, &f.aes, 50);
        let a = ix
            .accounts
            .iter_mut()
            .find(|a| a.pubkey == f.owner.pubkey())
            .unwrap();
        if missing {
            a.is_signer = false;
        } else {
            a.pubkey = f.mint.payer.pubkey();
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
        assert_eq!(f.mint.svm.get_account(&f.ata).unwrap().data, before);
    }
}
