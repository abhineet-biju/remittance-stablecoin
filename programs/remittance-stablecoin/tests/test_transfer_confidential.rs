mod common;
#[path = "common/confidential.rs"]
mod confidential;
#[path = "common/lifecycle.rs"]
mod lifecycle;
use {
    anchor_lang::prelude::Clock,
    anchor_spl::token_2022::spl_token_2022::{self, extension::transfer_fee},
    common::send,
    lifecycle::*,
    solana_signer::Signer,
};

#[test]
fn completes_the_confidential_lifecycle_with_encrypted_fees_and_withdrawal() {
    let mut f = setup(10_000_000);
    let bob = recipient(&mut f);
    deposit(&mut f, 10_000_000);
    apply(&mut f, 10_000_000);
    let mint_before = f.mint.svm.get_account(&f.mint.mint.pubkey()).unwrap().data;
    let prepared = prepare_transfer(&mut f, &bob, 3_000_001, None);
    let result = send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[prepared.instruction.clone()],
        &[&f.owner],
    )
    .unwrap();
    assert!(
        result.logs.iter().any(|l| l.contains("TransferWithFee")),
        "{result:?}"
    );
    assert_balances(&f, 0, 0, 6_999_999);
    assert_eq!(
        balances(&f.mint, bob.ata, &bob.elgamal, &bob.aes),
        (0, 2_970_000, 0)
    );
    assert_eq!(withheld_fee(&f, bob.ata), 30_001);
    assert_eq!(
        6_999_999 + 2_970_000 + withheld_fee(&f, bob.ata),
        10_000_000
    );
    let source_before = f.mint.svm.get_account(&f.ata).unwrap().data;
    let destination_before = f.mint.svm.get_account(&bob.ata).unwrap().data;
    f.mint.svm.expire_blockhash();
    assert!(send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[prepared.instruction],
        &[&f.owner]
    )
    .is_err());
    assert_eq!(f.mint.svm.get_account(&f.ata).unwrap().data, source_before);
    assert_eq!(
        f.mint.svm.get_account(&bob.ata).unwrap().data,
        destination_before
    );
    close_proofs(&mut f.mint, &f.owner, &prepared.contexts);
    let apply = apply_instruction(&f.mint, bob.owner.pubkey(), bob.ata, &bob.aes, 2_970_000);
    send(&mut f.mint.svm, &f.mint.payer, &[apply], &[&bob.owner]).unwrap();
    let withdraw = prepare_withdraw(
        &mut f.mint,
        &bob.owner,
        bob.ata,
        &bob.elgamal,
        &bob.aes,
        2_970_000,
    );
    send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[withdraw.instruction],
        &[&bob.owner],
    )
    .unwrap();
    close_proofs(&mut f.mint, &bob.owner, &withdraw.contexts);
    assert_eq!(
        balances(&f.mint, bob.ata, &bob.elgamal, &bob.aes),
        (2_970_000, 0, 0)
    );
    assert_eq!(withheld_fee(&f, bob.ata), 30_001);
    assert_eq!(
        f.mint.svm.get_account(&f.mint.mint.pubkey()).unwrap().data,
        mint_before
    );
}
#[test]
fn honors_fee_caps_and_rejects_proofs_using_an_incorrect_fee_rate() {
    let mut f = setup(10_000_000);
    let bob = recipient(&mut f);
    deposit(&mut f, 10_000_000);
    apply(&mut f, 10_000_000);
    let wrong = prepare_transfer(&mut f, &bob, 1_000_000, Some((200, 50_000)));
    let before = f.mint.svm.get_account(&f.ata).unwrap().data;
    assert!(send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[wrong.instruction],
        &[&f.owner]
    )
    .is_err());
    assert_eq!(f.mint.svm.get_account(&f.ata).unwrap().data, before);
    let capped = prepare_transfer(&mut f, &bob, 10_000_000, None);
    send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[capped.instruction],
        &[&f.owner],
    )
    .unwrap();
    assert_balances(&f, 0, 0, 0);
    assert_eq!(
        balances(&f.mint, bob.ata, &bob.elgamal, &bob.aes),
        (0, 9_950_000, 0)
    );
    assert_eq!(withheld_fee(&f, bob.ata), 50_000);
}
#[test]
fn rejects_old_fee_proofs_after_an_epoch_change_then_accepts_new_proofs() {
    let mut f = setup(1_000_000);
    let bob = recipient(&mut f);
    deposit(&mut f, 1_000_000);
    apply(&mut f, 1_000_000);
    let stale = prepare_transfer(&mut f, &bob, 100_000, None);
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
    let mut clock = f.mint.svm.get_sysvar::<Clock>();
    clock.epoch += 2;
    f.mint.svm.set_sysvar(&clock);
    let before = f.mint.svm.get_account(&f.ata).unwrap().data;
    assert!(send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[stale.instruction],
        &[&f.owner]
    )
    .is_err());
    assert_eq!(f.mint.svm.get_account(&f.ata).unwrap().data, before);
    let current = prepare_transfer(&mut f, &bob, 100_000, None);
    send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[current.instruction],
        &[&f.owner],
    )
    .unwrap();
    assert_eq!(withheld_fee(&f, bob.ata), 2_500);
    assert_eq!(
        balances(&f.mint, bob.ata, &bob.elgamal, &bob.aes),
        (0, 97_500, 0)
    );
}
#[test]
fn rejects_missing_signature_wrong_recipient_and_wrong_proof_type() {
    let mut f = setup(100);
    let bob = recipient(&mut f);
    let carol = recipient(&mut f);
    deposit(&mut f, 100);
    apply(&mut f, 100);
    let prepared = prepare_transfer(&mut f, &bob, 10, None);
    let before = f.mint.svm.get_account(&f.ata).unwrap().data;
    let mut unsigned = prepared.instruction.clone();
    unsigned
        .accounts
        .iter_mut()
        .find(|a| a.pubkey == f.owner.pubkey())
        .unwrap()
        .is_signer = false;
    assert!(send(&mut f.mint.svm, &f.mint.payer, &[unsigned], &[]).is_err());
    let mut wrong_recipient = prepared.instruction.clone();
    wrong_recipient
        .accounts
        .iter_mut()
        .find(|a| a.pubkey == bob.ata)
        .unwrap()
        .pubkey = carol.ata;
    assert!(send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[wrong_recipient],
        &[&f.owner]
    )
    .is_err());
    let mut wrong_context = prepared.instruction;
    wrong_context
        .accounts
        .iter_mut()
        .find(|a| a.pubkey == prepared.contexts[0])
        .unwrap()
        .pubkey = prepared.contexts[4];
    assert!(send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[wrong_context],
        &[&f.owner]
    )
    .is_err());
    assert_eq!(f.mint.svm.get_account(&f.ata).unwrap().data, before);
    assert_eq!(
        balances(&f.mint, bob.ata, &bob.elgamal, &bob.aes),
        (0, 0, 0)
    );
    assert_eq!(
        balances(&f.mint, carol.ata, &carol.elgamal, &carol.aes),
        (0, 0, 0)
    );
}
#[test]
fn rejects_stale_balance_proofs_and_frozen_recipients() {
    let mut f = setup(101);
    let bob = recipient(&mut f);
    deposit(&mut f, 100);
    apply(&mut f, 100);
    let stale = prepare_transfer(&mut f, &bob, 10, None);
    deposit(&mut f, 1);
    apply(&mut f, 101);
    let before = f.mint.svm.get_account(&f.ata).unwrap().data;
    assert!(send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[stale.instruction],
        &[&f.owner]
    )
    .is_err());
    let current = prepare_transfer(&mut f, &bob, 10, None);
    let freeze = spl_token_2022::instruction::freeze_account(
        &spl_token_2022::id(),
        &bob.ata,
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
    assert!(send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[current.instruction],
        &[&f.owner]
    )
    .is_err());
    assert_eq!(f.mint.svm.get_account(&f.ata).unwrap().data, before);
}
