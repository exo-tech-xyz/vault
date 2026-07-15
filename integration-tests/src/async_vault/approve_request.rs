use anchor_spl::{
    associated_token::get_associated_token_address_with_program_id, token, token_2022,
};
use async_vault_client::{
    lite::SendTransaction, sdk::program_id, ApproveRequestBuilder, CancelRequestBuilder,
    CreateDepositRequestBuilder, CreateRedeemRequestBuilder,
    InitializeVaultBuilder as InitializeAsyncVaultBuilder, Request, RequestArgs, RequestState,
    UpdateVaultBuilder as UpdateVaultAsyncBuilder, UpdateVaultNavBuilder, Vault,
};
use borsh::BorshSerialize;
use litesvm::LiteSVM;
use solana_sdk::{
    account::ReadableAccount, clock::Clock, pubkey::Pubkey, signature::Keypair, signer::Signer,
    transaction::Transaction,
};
use test_case::test_case;

use crate::{
    async_helper_functions::{
        approve_request_args, assert_error_code, get_token_account_amount, helper_mint_to,
        set_share_balance, set_up_async_vault, set_vault_total_asset_balance,
    },
    async_vault::constants::{
        APPROVAL_REQUEST_MISMATCH, INSUFFICIENT_DEPOSIT_AMOUNT, INVALID_ASSET_MINT_EXTENSIONS,
        NAV_IS_NOT_SET, PAUSED_VAULT, REQUEST_NOT_PENDING, UNAUTHORIZED_SIGNER,
    },
};

fn setup(
    svm: &mut LiteSVM,
    nav: u128,
) -> (
    Keypair, // authority
    Keypair, // mint_authority
    Keypair, // asset_mint
    Keypair, // share_mint
    Keypair, // user
    Pubkey,  // reserve_pubkey
    Pubkey,  // vault_pubkey
    Pubkey,  // pending_vault_pubkey
    Pubkey,  // user_asset_account
    Pubkey,  // user_share_account
) {
    let (
        authority,
        _payer,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault(svm, token::ID, None, token::ID, 1_000_000_000);

    InitializeAsyncVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");

    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(nav)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[&authority])
        .expect("update nav should succeed");

    let user_asset_account = get_associated_token_address_with_program_id(
        &user.pubkey(),
        &asset_mint.pubkey(),
        &token::ID,
    );

    (
        authority,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        user_asset_account,
        user_share_account,
    )
}

#[test_case(200, 1_000_000, 5_000_000_000_000 ; "nav=200_10-9")]
#[test_case(200_000_000_000, 1_000_000, 5_000 ; "nav=200")]
fn test_approve_deposit_request_success(
    nav: u128,
    deposit_amount: u64,
    expected_deposit_shares: u64,
) {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        user_asset_account,
        _user_share_account,
    ) = setup(&mut svm, nav);

    let request_keypair = Keypair::new();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(spl_token::ID)
        .args(RequestArgs {
            amount: deposit_amount,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("create deposit request should succeed");

    let pending_before = get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap());
    let reserve_before = get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap());

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &request_keypair.pubkey());
    ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .request(request_keypair.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("approve_request should succeed");

    let vault_after = Vault::from_bytes(
        svm.get_account(&vault_pubkey)
            .expect("vault should exist")
            .data(),
    )
    .unwrap();
    assert_eq!(
        vault_after.pending_async_requests, 1,
        "the request remains outstanding until the user claims shares"
    );
    assert_eq!(
        vault_after.total_asset_balance, deposit_amount,
        "total_asset_balance should be incremented by deposit amount"
    );

    let request_after = Request::from_bytes(
        svm.get_account(&request_keypair.pubkey())
            .expect("request should exist")
            .data(),
    )
    .unwrap();
    assert_eq!(request_after.request_state, RequestState::Claimable);
    assert_eq!(request_after.price, nav);
    assert_eq!(
        request_after.amount, expected_deposit_shares,
        "request.amount should be updated to claimable shares"
    );

    assert_eq!(
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap()),
        pending_before - deposit_amount,
        "pending_vault should be drained"
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        reserve_before + deposit_amount,
        "reserve should receive the deposited assets"
    );
}

#[test_case(200, 5_000_000_000_000, 1_000_000 ; "nav=200_10-9")]
#[test_case(200_000_000_000, 5_000, 1_000_000 ; "nav=200")]
fn test_approve_redeem_request_success(nav: u128, redeem_amount: u64, expected_redeem_assets: u64) {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        _user_asset_account,
        user_share_account,
    ) = setup(&mut svm, nav);

    // Fund reserve with assets to cover the redeem payout
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        expected_redeem_assets,
        &token::ID,
    );

    set_vault_total_asset_balance(&mut svm, vault_pubkey, expected_redeem_assets);

    // Give the user shares to redeem
    set_share_balance(
        &mut svm,
        &user_share_account,
        &share_mint.pubkey(),
        redeem_amount,
    );

    let request_keypair = Keypair::new();
    CreateRedeemRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_share_account(user_share_account)
        .share_token_program(spl_token::ID)
        .args(RequestArgs {
            amount: redeem_amount,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("create redeem request should succeed");

    let reserve_before = get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap());
    let pending_before = get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap());

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &request_keypair.pubkey());
    ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .request(request_keypair.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("approve_request should succeed");

    let vault_after = Vault::from_bytes(
        svm.get_account(&vault_pubkey)
            .expect("vault should exist")
            .data(),
    )
    .unwrap();
    assert_eq!(
        vault_after.pending_async_requests, 1,
        "the request remains outstanding until the user claims assets"
    );
    assert_eq!(
        vault_after.total_asset_balance, 0,
        "total_asset_balance should be decremented by redeemed assets"
    );

    let request_after = Request::from_bytes(
        svm.get_account(&request_keypair.pubkey())
            .expect("request should exist")
            .data(),
    )
    .unwrap();
    assert_eq!(request_after.request_state, RequestState::Claimable);
    assert_eq!(request_after.price, nav);
    assert_eq!(
        request_after.amount, expected_redeem_assets,
        "request.amount should be updated to claimable assets"
    );

    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        reserve_before - expected_redeem_assets,
        "reserve should be drained by redeemed assets"
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap()),
        pending_before + expected_redeem_assets,
        "pending_vault should receive the redeemed assets"
    );
}

#[test_case(false, true, false, 1_000_000, UNAUTHORIZED_SIGNER ; "unauthorized signer")]
#[test_case(true, false, false, 1_000_000, PAUSED_VAULT ; "paused vault")]
#[test_case(false, false, true, 1_000_000, REQUEST_NOT_PENDING ; "request not in pending state")]
#[test_case(false, false, false, 1_000_000, NAV_IS_NOT_SET ; "nav not set")]
fn test_approve_request_fails(
    pause_vault: bool,
    use_wrong_signer: bool,
    override_to_claimable: bool,
    deposit_amount: u64,
    expected_error_code: u32,
) {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let user_amount = 1_000_000_000;
    let (
        authority,
        _payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault(&mut svm, token::ID, None, token::ID, user_amount);
    InitializeAsyncVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");

    let user_token_account = get_associated_token_address_with_program_id(
        &user.pubkey(),
        &asset_mint.pubkey(),
        &token::ID,
    );

    let request_keypair = Keypair::new();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_token_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(spl_token::ID)
        .args(RequestArgs {
            amount: deposit_amount,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("create deposit request should succeed");

    if override_to_claimable {
        let mut account = svm.get_account(&request_keypair.pubkey()).unwrap();
        let mut request = Request::from_bytes(account.data()).unwrap();
        request.request_state = RequestState::Claimable;
        let mut buf = Vec::new();
        request.serialize(&mut buf).unwrap();
        account.data = buf;
        svm.set_account(request_keypair.pubkey(), account).unwrap();
    }

    if pause_vault {
        UpdateVaultAsyncBuilder::new()
            .authority(authority.pubkey())
            .share_mint(share_mint.pubkey())
            .paused(true)
            .vault(vault_pubkey)
            .instruction()
            .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
            .expect("pause vault should succeed");
    }

    let (signer, authority_key) = if use_wrong_signer {
        (&user, user.pubkey())
    } else {
        (&authority, authority.pubkey())
    };

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &request_keypair.pubkey());
    let ix = ApproveRequestBuilder::new()
        .authority(authority_key)
        .vault(vault_pubkey)
        .request(request_keypair.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .instruction();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&signer.pubkey()),
        &[signer],
        svm.latest_blockhash(),
    );
    let err = svm.send_transaction(tx).unwrap_err();
    assert_error_code(&err, expected_error_code, "");
}

#[test]
fn test_stale_approval_rejected_on_recreated_request() {
    const NAV: u128 = 200_000_000_000;
    let original_amount = 1_000_000u64;
    let replacement_amount = 600_000u64;

    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        user_asset_account,
        _user_share_account,
    ) = setup(&mut svm, NAV);

    let request_keypair = Keypair::new();
    let create_deposit = |svm: &mut LiteSVM, amount: u64| {
        CreateDepositRequestBuilder::new()
            .user(user.pubkey())
            .asset_mint(asset_mint.pubkey())
            .share_mint(share_mint.pubkey())
            .request(request_keypair.pubkey())
            .vault(vault_pubkey)
            .user_token_account(user_asset_account)
            .pending_vault(pending_vault_pubkey)
            .asset_token_program(spl_token::ID)
            .args(RequestArgs {
                amount,
                operator: None,
            })
            .instruction()
            .send_transaction(svm, &user.pubkey(), &[&user, &request_keypair])
            .expect("create deposit request should succeed");
    };

    create_deposit(&mut svm, original_amount);

    let (stale_owner, stale_type, stale_amount, stale_created_at, stale_nav_version) =
        approve_request_args(&svm, &request_keypair.pubkey());

    CancelRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_token_account(Some(user_asset_account))
        .asset_pending_vault(Some(pending_vault_pubkey))
        .asset_token_program(Some(token::ID))
        .user_share_account(None)
        .share_token_program(None)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("cancel should succeed");
    create_deposit(&mut svm, replacement_amount);

    let stale_err = ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .request(request_keypair.pubkey())
        .owner(stale_owner)
        .request_type(stale_type)
        .amount(stale_amount)
        .created_at(stale_created_at)
        .nav_update_version(stale_nav_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&stale_err, APPROVAL_REQUEST_MISMATCH, "");

    let request_after_stale = Request::from_bytes(
        svm.get_account(&request_keypair.pubkey())
            .expect("replacement request should still exist")
            .data(),
    )
    .unwrap();
    assert_eq!(
        request_after_stale.request_state,
        RequestState::Pending,
        "replacement request must remain pending after a rejected stale approval"
    );

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &request_keypair.pubkey());
    ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .request(request_keypair.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("fresh approval matching the live request should succeed");

    let request_after_fresh = Request::from_bytes(
        svm.get_account(&request_keypair.pubkey())
            .expect("request should exist")
            .data(),
    )
    .unwrap();
    assert_eq!(request_after_fresh.request_state, RequestState::Claimable);
    assert_eq!(
        request_after_fresh.amount,
        replacement_amount / 200,
        "fresh approval settles the replacement instance, not the stale one"
    );
}

#[test]
fn test_approve_rejected_when_transfer_fee_reenabled() {
    const USER_AMOUNT: u64 = 2_000_000;
    const DEPOSIT_AMOUNT: u64 = 1_000_000;
    const NAV: u128 = 1_000_000_000;
    const FEE_BPS: u16 = 100;

    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault(&mut svm, token_2022::ID, Some(0), token::ID, USER_AMOUNT);

    InitializeAsyncVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");

    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(NAV)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("update nav should succeed");

    let user_asset_account = get_associated_token_address_with_program_id(
        &user.pubkey(),
        &asset_mint.pubkey(),
        &token_2022::ID,
    );

    let request_keypair = Keypair::new();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token_2022::ID)
        .args(RequestArgs {
            amount: DEPOSIT_AMOUNT,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("create deposit request should succeed while transfer fee is zero");

    let set_fee_ix =
        token_2022::spl_token_2022::extension::transfer_fee::instruction::set_transfer_fee(
            &token_2022::ID,
            &asset_mint.pubkey(),
            &mint_authority.pubkey(),
            &[],
            FEE_BPS,
            u64::MAX,
        )
        .unwrap();
    let set_fee_tx = Transaction::new_signed_with_payer(
        &[set_fee_ix],
        Some(&mint_authority.pubkey()),
        &[&mint_authority],
        svm.latest_blockhash(),
    );
    svm.send_transaction(set_fee_tx)
        .expect("set_transfer_fee should succeed");

    let mut clock = svm.get_sysvar::<Clock>();
    clock.epoch += 2;
    svm.set_sysvar(&clock);

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &request_keypair.pubkey());
    let err = ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .request(request_keypair.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token_2022::ID)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&err, INVALID_ASSET_MINT_EXTENSIONS, "");
}

#[test]
fn test_approve_deposit_rejected_when_shares_round_to_zero() {
    const NAV: u128 = 200_000_000_000;
    const DUST_DEPOSIT: u64 = 1;

    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        user_asset_account,
        _user_share_account,
    ) = setup(&mut svm, NAV);

    let request_keypair = Keypair::new();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(spl_token::ID)
        .args(RequestArgs {
            amount: DUST_DEPOSIT,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("dust deposit request should succeed");

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &request_keypair.pubkey());
    let err = ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .request(request_keypair.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(
        &err,
        INSUFFICIENT_DEPOSIT_AMOUNT,
        "Deposit amount too small.",
    );

    let request_after = Request::from_bytes(
        svm.get_account(&request_keypair.pubkey())
            .expect("request should still exist")
            .data(),
    )
    .unwrap();
    assert_eq!(request_after.request_state, RequestState::Pending);
    assert_eq!(
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap()),
        DUST_DEPOSIT
    );
}
