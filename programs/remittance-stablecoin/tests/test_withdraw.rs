mod common;
#[path = "common/confidential.rs"]
mod confidential;
#[path = "common/lifecycle.rs"]
mod lifecycle;
use {
    anchor_lang::InstructionData, common::send, lifecycle::*, solana_signer::Signer,
    solana_zk_sdk::encryption::elgamal::ElGamalCiphertext,
    spl_token_confidential_transfer_proof_generation::withdraw::withdraw_proof_data,
};

#[test]
fn withdraws_partial_then_full_available_funds_and_closes_proofs() {
    let mut f = setup(15_000_000);
    deposit(&mut f, 10_000_000);
    apply(&mut f, 10_000_000);
    deposit(&mut f, 2_000_000);
    let mint_before = f.mint.svm.get_account(&f.mint.mint.pubkey()).unwrap().data;
    let prepared = prepare_withdraw(&mut f.mint, &f.owner, f.ata, &f.elgamal, &f.aes, 3_000_001);
    send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[prepared.instruction.clone()],
        &[&f.owner],
    )
    .unwrap();
    assert_balances(&f, 6_000_001, 2_000_000, 6_999_999);
    let before = f.mint.svm.get_account(&f.ata).unwrap().data;
    f.mint.svm.expire_blockhash();
    assert!(send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[prepared.instruction],
        &[&f.owner]
    )
    .is_err());
    assert_eq!(f.mint.svm.get_account(&f.ata).unwrap().data, before);
    close_proofs(&mut f.mint, &f.owner, &prepared.contexts);
    // Apply the remaining pending funds before withdrawing the full balance.
    apply(&mut f, 8_999_999);
    let prepared = prepare_withdraw(&mut f.mint, &f.owner, f.ata, &f.elgamal, &f.aes, 8_999_999);
    send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[prepared.instruction],
        &[&f.owner],
    )
    .unwrap();
    assert_balances(&f, 15_000_000, 0, 0);
    close_proofs(&mut f.mint, &f.owner, &prepared.contexts);
    assert_eq!(
        f.mint.svm.get_account(&f.mint.mint.pubkey()).unwrap().data,
        mint_before
    );
}
#[test]
fn rejects_mismatched_amount_missing_signature_and_wrong_proof_context() {
    let mut f = setup(100);
    deposit(&mut f, 100);
    apply(&mut f, 100);
    let prepared = prepare_withdraw(&mut f.mint, &f.owner, f.ata, &f.elgamal, &f.aes, 10);
    let before = f.mint.svm.get_account(&f.ata).unwrap().data;
    let mut mismatch = prepared.instruction.clone();
    mismatch.data = remittance_stablecoin::instruction::Withdraw {
        amount: 20,
        new_decryptable_available_balance: f.aes.encrypt(80).to_bytes(),
    }
    .data();
    assert!(send(&mut f.mint.svm, &f.mint.payer, &[mismatch], &[&f.owner]).is_err());
    let mut unsigned = prepared.instruction.clone();
    unsigned
        .accounts
        .iter_mut()
        .find(|a| a.pubkey == f.owner.pubkey())
        .unwrap()
        .is_signer = false;
    assert!(send(&mut f.mint.svm, &f.mint.payer, &[unsigned], &[]).is_err());
    let mut wrong = prepared.instruction;
    wrong
        .accounts
        .iter_mut()
        .find(|a| a.pubkey == prepared.contexts[0])
        .unwrap()
        .pubkey = prepared.contexts[1];
    assert!(send(&mut f.mint.svm, &f.mint.payer, &[wrong], &[&f.owner]).is_err());
    assert_eq!(f.mint.svm.get_account(&f.ata).unwrap().data, before);
}
#[test]
fn pending_funds_cannot_cover_a_withdrawal_until_applied() {
    let mut f = setup(100);
    deposit(&mut f, 100);
    let e = extension(&f.mint, f.ata);
    assert!(withdraw_proof_data(
        &ElGamalCiphertext::from_bytes(bytemuck::bytes_of(&e.available_balance)).unwrap(),
        0,
        1,
        &f.elgamal
    )
    .is_err());
    apply(&mut f, 100);
    let prepared = prepare_withdraw(&mut f.mint, &f.owner, f.ata, &f.elgamal, &f.aes, 100);
    send(
        &mut f.mint.svm,
        &f.mint.payer,
        &[prepared.instruction],
        &[&f.owner],
    )
    .unwrap();
    assert_balances(&f, 100, 0, 0);
}
