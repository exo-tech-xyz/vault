use anchor_spl::{associated_token::get_associated_token_address_with_program_id, token};
use async_vault_client::{
    lite::SendTransaction, sdk::program_id, ApproveRequestBuilder, ClaimBuilder,
    CreateDepositRequestBuilder, CreateRedeemRequestBuilder,
    InitializeVaultBuilder as InitializeAsyncVaultBuilder, RequestArgs,
    UpdateVaultBuilder as UpdateVaultAsyncBuilder, UpdateVaultNavBuilder, Vault,
};
use litesvm::LiteSVM;
use solana_sdk::{
    account::ReadableAccount, pubkey::Pubkey, signature::Keypair, signer::Signer,
    transaction::Transaction,
};
use test_case::test_case;

use crate::{
    async_helper_functions::{
        approve_request_args, assert_error_code, get_mint_supply, get_token_account_amount,
        helper_mint_to, set_share_balance, set_up_async_vault, set_vault_total_asset_balance,
    },
    async_vault::constants::{PAUSED_VAULT, REQUEST_NOT_CLAIMABLE, UNAUTHORIZED_SIGNER},
};

const LITESVM_TX_COST: u64 = 5000;

fn setup(
    svm: &mut LiteSVM,
    nav: u128,
) -> (
    Keypair, // authority
    Keypair, // mint_authority
    Keypair, // asset_mint
    Keypair, // share_mint
    Keypair, // user
    Keypair, // operator
    Pubkey,  // reserve_pubkey (vault_token_account)
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
        operator,
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
        operator,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        user_asset_account,
        user_share_account,
    )
}

#[test_case(false, 1_000_000, 1_000_000, 1_000_000_000 ; "owner claims deposit")]
#[test_case(true,  1_000_000, 1_000_000, 1_000_000_000 ; "operator claims deposit")]
fn test_claim_deposit_success(
    use_operator: bool,
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
        operator,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        _user_asset_account,
        user_share_account,
    ) = setup(&mut svm, nav);

    let request_keypair = Keypair::new();
    let operator_pubkey = use_operator.then_some(operator.pubkey());

    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_token_account(_user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(spl_token::ID)
        .args(RequestArgs {
            amount: deposit_amount,
            operator: operator_pubkey,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("create deposit request should succeed");

    // Approve: assets move pending → reserve, request.amount set to shares
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

    // Snapshot balances after approve (before claim)
    let request_account_before = svm.get_account(&request_keypair.pubkey()).unwrap();
    let request_owner_account_before = svm.get_account(&user.pubkey()).unwrap();
    let user_shares_before =
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap());
    let share_supply_before = get_mint_supply(&svm.get_account(&share_mint.pubkey()).unwrap());

    let claimer = if use_operator { &operator } else { &user };
    ClaimBuilder::new()
        .user(claimer.pubkey())
        .owner(user.pubkey())
        .vault(vault_pubkey)
        .request(request_keypair.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .pending_vault(None)
        .user_share_account(Some(user_share_account))
        .user_asset_account(None)
        .asset_token_program(spl_token::ID)
        .share_token_program(Some(spl_token::ID))
        .instruction()
        .send_transaction(&mut svm, &claimer.pubkey(), &[claimer])
        .expect("claim_request should succeed");

    let request_owner_account_after = svm.get_account(&user.pubkey()).unwrap();

    assert!(
        svm.get_account(&request_keypair.pubkey()).is_none(),
        "request account should be closed after claim"
    );
    let vault_after = Vault::from_bytes(
        svm.get_account(&vault_pubkey)
            .expect("vault should exist")
            .data(),
    )
    .unwrap();
    assert_eq!(
        vault_after.pending_async_requests, 0,
        "claim should settle the outstanding request"
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        user_shares_before + expected_deposit_shares,
        "user should receive minted shares"
    );
    assert_eq!(
        get_mint_supply(&svm.get_account(&share_mint.pubkey()).unwrap()),
        share_supply_before + expected_deposit_shares,
        "share mint supply should increase by minted shares"
    );
    // Account for TX fee when User instead of Operator
    let expected_lamports = if use_operator {
        request_owner_account_before.lamports + request_account_before.lamports
    } else {
        request_owner_account_before.lamports + request_account_before.lamports - LITESVM_TX_COST
    };
    assert_eq!(
        request_owner_account_after.lamports, expected_lamports,
        "Rent is sent to request owner"
    );
}

#[test_case(false, 1_000_000, 1_000_000_000, 1_000_000 ; "owner claims redeem")]
#[test_case(true,  1_000_000, 1_000_000_000, 1_000_000 ; "operator claims redeem")]
fn test_claim_redeem_success(
    use_operator: bool,
    nav: u128,
    redeem_amount: u64,
    expected_redeem_assets: u64,
) {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        operator,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        user_asset_account,
        user_share_account,
    ) = setup(&mut svm, nav);

    // Fund the reserve so approve can transfer out
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        expected_redeem_assets,
        &token::ID,
    );

    // Set vault.total_asset_balance to match the funded reserve
    set_vault_total_asset_balance(&mut svm, vault_pubkey, expected_redeem_assets);

    // Give the user shares to redeem
    set_share_balance(
        &mut svm,
        &user_share_account,
        &share_mint.pubkey(),
        redeem_amount,
    );

    let request_keypair = Keypair::new();
    let operator_pubkey = use_operator.then_some(operator.pubkey());

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
            operator: operator_pubkey,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("create redeem request should succeed");

    // Approve: assets move reserve → pending_vault, request.amount set to assets
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

    // Snapshot balances after approve (before claim)
    let request_account_before = svm.get_account(&request_keypair.pubkey()).unwrap();
    let request_owner_account_before = svm.get_account(&user.pubkey()).unwrap();
    let pending_vault_before =
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap());
    let user_assets_before =
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap());

    let claimer = if use_operator { &operator } else { &user };
    ClaimBuilder::new()
        .user(claimer.pubkey())
        .owner(user.pubkey())
        .vault(vault_pubkey)
        .request(request_keypair.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .pending_vault(Some(pending_vault_pubkey))
        .user_share_account(None)
        .user_asset_account(Some(user_asset_account))
        .asset_token_program(spl_token::ID)
        .share_token_program(None)
        .instruction()
        .send_transaction(&mut svm, &claimer.pubkey(), &[claimer])
        .expect("claim_request should succeed");

    let request_owner_account_after = svm.get_account(&user.pubkey()).unwrap();

    assert!(
        svm.get_account(&request_keypair.pubkey()).is_none(),
        "request account should be closed after claim"
    );
    let vault_after = Vault::from_bytes(
        svm.get_account(&vault_pubkey)
            .expect("vault should exist")
            .data(),
    )
    .unwrap();
    assert_eq!(
        vault_after.pending_async_requests, 0,
        "claim should settle the outstanding request"
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap()),
        user_assets_before + expected_redeem_assets,
        "user should receive assets"
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap()),
        pending_vault_before - expected_redeem_assets,
        "pending_vault should be drained by claim"
    );
    // Account for TX fee when User instead of Operator
    let expected_lamports = if use_operator {
        request_owner_account_before.lamports + request_account_before.lamports
    } else {
        request_owner_account_before.lamports + request_account_before.lamports - LITESVM_TX_COST
    };
    assert_eq!(
        request_owner_account_after.lamports, expected_lamports,
        "Rent is sent to request owner"
    );
}

#[test_case(true,  false, false, 1_000_000, UNAUTHORIZED_SIGNER ; "unauthorized signer")]
#[test_case(false, true,  false, 1_000_000, PAUSED_VAULT ; "paused vault")]
#[test_case(false, false, true,  1_000_000, REQUEST_NOT_CLAIMABLE ; "request not claimable")]
fn test_claim_fails(
    use_wrong_signer: bool,
    pause_vault: bool,
    skip_approve: bool,
    deposit_amount: u64,
    expected_error_code: u32,
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
        _operator,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        user_asset_account,
        user_share_account,
    ) = setup(&mut svm, 1_000_000);

    let request_keypair = Keypair::new();
    let ix = CreateDepositRequestBuilder::new()
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
        .instruction();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&user.pubkey()),
        &[&user, &request_keypair],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx)
        .expect("create deposit request should succeed");

    if !skip_approve {
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

    let wrong_signer = Keypair::new();
    svm.airdrop(&wrong_signer.pubkey(), 1_000_000_000).unwrap();
    let claimer = if use_wrong_signer {
        &wrong_signer
    } else {
        &user
    };

    let result = ClaimBuilder::new()
        .user(claimer.pubkey())
        .owner(user.pubkey())
        .vault(vault_pubkey)
        .request(request_keypair.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .pending_vault(None)
        .user_share_account(Some(user_share_account))
        .user_asset_account(None)
        .asset_token_program(spl_token::ID)
        .share_token_program(Some(spl_token::ID))
        .instruction()
        .send_transaction(&mut svm, &claimer.pubkey(), &[claimer]);

    assert!(result.is_err(), "claim should fail");
    assert_error_code(&result.unwrap_err(), expected_error_code, "");
}
