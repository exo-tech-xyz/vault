use anchor_spl::{
    associated_token::get_associated_token_address_with_program_id,
    token::{self},
    token_2022,
};
use async_vault_client::{
    lite::SendTransaction, sdk::program_id, ApproveRequestBuilder, CancelRequestBuilder,
    ClaimBuilder, CloseVaultBuilder, CreateDepositRequestBuilder, CreateRedeemRequestBuilder,
    InitializePausableRedemptionsBuilder, InitializeRedemptionQueueBuilder,
    InitializeVaultBuilder as InitializeAsyncVaultBuilder, RequestArgs, ShutdownVaultBuilder,
    UpdatePausableRedemptionsBuilder, UpdateVaultBuilder, UpdateVaultNavBuilder, Vault,
    WithdrawAssetsBuilder,
};
use litesvm::LiteSVM;
use solana_sdk::{account::ReadableAccount, pubkey::Pubkey, signature::Keypair, signer::Signer};
use test_case::test_case;

use crate::{
    async_helper_functions::{
        approve_request_args, assert_error_code, helper_mint_to, set_share_balance,
        set_up_async_vault, set_vault_total_asset_balance,
    },
    async_vault::constants::{
        REDEMPTIONS_PAUSED, UNAUTHORIZED_SIGNER, VAULT_ALREADY_CLOSING,
        VAULT_HAS_PENDING_ASYNC_REQUESTS, VAULT_IS_CLOSING,
    },
};

const NAV: u128 = 1_000_000_000;
const EXIT_AMOUNT: u64 = 1_000_000;

#[allow(clippy::type_complexity)]
fn setup(
    asset_token_program: Pubkey,
    share_token_program: Pubkey,
    pausable_redemptions: Option<bool>,
    with_redemption_queue: bool,
) -> (
    LiteSVM,
    Keypair,
    Keypair,
    Keypair,
    Keypair,
    Keypair,
    solana_sdk::pubkey::Pubkey,
    solana_sdk::pubkey::Pubkey,
    solana_sdk::pubkey::Pubkey,
    solana_sdk::pubkey::Pubkey,
    solana_sdk::pubkey::Pubkey,
) {
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
        reserve,
        vault,
        pending_vault,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault(
        &mut svm,
        asset_token_program,
        None,
        share_token_program,
        EXIT_AMOUNT,
    );

    if let Some(paused) = pausable_redemptions {
        InitializePausableRedemptionsBuilder::new()
            .payer(authority.pubkey())
            .authority(authority.pubkey())
            .vault(vault)
            .paused(paused)
            .instruction()
            .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
            .expect("initialize pausable redemptions should succeed");
    }

    if with_redemption_queue {
        InitializeRedemptionQueueBuilder::new()
            .payer(authority.pubkey())
            .authority(authority.pubkey())
            .vault(vault)
            .instruction()
            .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
            .expect("initialize redemption queue should succeed");
    }

    InitializeAsyncVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault)
        .updated_nav(NAV)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set NAV should succeed");

    let user_asset_account = get_associated_token_address_with_program_id(
        &user.pubkey(),
        &asset_mint.pubkey(),
        &asset_token_program,
    );

    (
        svm,
        authority,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve,
        pending_vault,
        vault,
        user_asset_account,
        user_share_account,
    )
}

fn shutdown(
    svm: &mut LiteSVM,
    authority: &Keypair,
    share_mint: solana_sdk::pubkey::Pubkey,
    vault: solana_sdk::pubkey::Pubkey,
) -> litesvm::types::TransactionResult {
    ShutdownVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint)
        .vault(vault)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
}

#[test_case(token::ID, token::ID, false ; "spl_token")]
#[test_case(token_2022::ID, token_2022::ID, false ; "token_2022")]
#[test_case(token::ID, token::ID, true ; "redemption_queue")]
fn shutdown_blocks_new_subscriptions_and_withdrawals_but_allows_redemption_exit(
    asset_token_program: Pubkey,
    share_token_program: Pubkey,
    with_redemption_queue: bool,
) {
    let (
        mut svm,
        authority,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve,
        pending_vault,
        vault,
        user_asset_account,
        user_share_account,
    ) = setup(
        asset_token_program,
        share_token_program,
        None,
        with_redemption_queue,
    );

    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve,
        &mint_authority,
        EXIT_AMOUNT,
        &asset_token_program,
    );
    set_vault_total_asset_balance(&mut svm, vault, EXIT_AMOUNT);
    set_share_balance(
        &mut svm,
        &user_share_account,
        &share_mint.pubkey(),
        EXIT_AMOUNT,
    );

    let pending_deposit = Keypair::new();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(pending_deposit.pubkey())
        .vault(vault)
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault)
        .asset_token_program(asset_token_program)
        .args(RequestArgs {
            amount: 1,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &pending_deposit])
        .expect("deposit request should exist before shutdown");

    shutdown(&mut svm, &authority, share_mint.pubkey(), vault).expect("shutdown should succeed");
    let vault_state = Vault::from_bytes(svm.get_account(&vault).unwrap().data()).unwrap();
    assert!(vault_state.closing);

    let blocked_deposit_request = Keypair::new();
    let deposit_err = CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(blocked_deposit_request.pubkey())
        .vault(vault)
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault)
        .asset_token_program(asset_token_program)
        .args(RequestArgs {
            amount: 1,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &blocked_deposit_request])
        .unwrap_err();
    assert_error_code(&deposit_err, VAULT_IS_CLOSING, "VaultIsClosing");

    let withdraw_err = WithdrawAssetsBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .vault(vault)
        .vault_token_account(reserve)
        .recipient_token_account(user_asset_account)
        .asset_token_program(asset_token_program)
        .amount(0)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&withdraw_err, VAULT_IS_CLOSING, "VaultIsClosing");

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &pending_deposit.pubkey());
    let pending_deposit_err = ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault)
        .request(pending_deposit.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault_token_account(reserve)
        .pending_vault(pending_vault)
        .asset_token_program(asset_token_program)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&pending_deposit_err, VAULT_IS_CLOSING, "VaultIsClosing");

    CancelRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(pending_deposit.pubkey())
        .vault(vault)
        .user_token_account(Some(user_asset_account))
        .asset_pending_vault(Some(pending_vault))
        .asset_token_program(Some(asset_token_program))
        .user_share_account(None)
        .share_token_program(None)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("pending deposits should be cancelable during shutdown");

    let redeem_request = Keypair::new();
    CreateRedeemRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(redeem_request.pubkey())
        .vault(vault)
        .user_share_account(user_share_account)
        .share_token_program(share_token_program)
        .args(RequestArgs {
            amount: EXIT_AMOUNT,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &redeem_request])
        .expect("redemption request should remain available during shutdown");

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &redeem_request.pubkey());
    ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault)
        .request(redeem_request.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault_token_account(reserve)
        .pending_vault(pending_vault)
        .asset_token_program(asset_token_program)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("redemption approval should remain available during shutdown");

    let close_before_claim_err = CloseVaultBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault)
        .reserve(reserve)
        .pending_vault(pending_vault)
        .authority_asset_token_account(get_associated_token_address_with_program_id(
            &authority.pubkey(),
            &asset_mint.pubkey(),
            &asset_token_program,
        ))
        .asset_token_program(asset_token_program)
        .share_token_program(share_token_program)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(
        &close_before_claim_err,
        VAULT_HAS_PENDING_ASYNC_REQUESTS,
        "VaultHasPendingAsyncRequests",
    );
    svm.expire_blockhash();

    ClaimBuilder::new()
        .user(user.pubkey())
        .owner(user.pubkey())
        .vault(vault)
        .request(redeem_request.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .pending_vault(Some(pending_vault))
        .user_share_account(None)
        .user_asset_account(Some(user_asset_account))
        .asset_token_program(asset_token_program)
        .share_token_program(None)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("redemption claim should remain available during shutdown");

    CloseVaultBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault)
        .reserve(reserve)
        .pending_vault(pending_vault)
        .authority_asset_token_account(get_associated_token_address_with_program_id(
            &authority.pubkey(),
            &asset_mint.pubkey(),
            &asset_token_program,
        ))
        .asset_token_program(asset_token_program)
        .share_token_program(share_token_program)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("final close should succeed after the exit is settled");

    assert!(svm.get_account(&vault).is_none());
}

#[test]
fn shutdown_is_authority_only_and_irreversible() {
    let (
        mut svm,
        authority,
        _mint_authority,
        _asset_mint,
        share_mint,
        _user,
        _reserve,
        _pending_vault,
        vault,
        _user_asset_account,
        _user_share_account,
    ) = setup(token::ID, token::ID, None, false);

    let unauthorized = Keypair::new();
    svm.airdrop(&unauthorized.pubkey(), 1_000_000_000).unwrap();
    let unauthorized_err =
        shutdown(&mut svm, &unauthorized, share_mint.pubkey(), vault).unwrap_err();
    assert_error_code(&unauthorized_err, UNAUTHORIZED_SIGNER, "UnauthorizedSigner");

    shutdown(&mut svm, &authority, share_mint.pubkey(), vault).expect("shutdown should succeed");
    svm.expire_blockhash();
    let duplicate_err = shutdown(&mut svm, &authority, share_mint.pubkey(), vault).unwrap_err();
    assert_error_code(&duplicate_err, VAULT_ALREADY_CLOSING, "VaultAlreadyClosing");
}

#[test]
fn shutdown_allows_claim_of_a_redemption_approved_before_shutdown() {
    let (
        mut svm,
        authority,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve,
        pending_vault,
        vault,
        user_asset_account,
        user_share_account,
    ) = setup(token::ID, token::ID, None, false);

    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve,
        &mint_authority,
        EXIT_AMOUNT,
        &token::ID,
    );
    set_vault_total_asset_balance(&mut svm, vault, EXIT_AMOUNT);
    set_share_balance(
        &mut svm,
        &user_share_account,
        &share_mint.pubkey(),
        EXIT_AMOUNT,
    );

    let redeem_request = Keypair::new();
    CreateRedeemRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(redeem_request.pubkey())
        .vault(vault)
        .user_share_account(user_share_account)
        .share_token_program(token::ID)
        .args(RequestArgs {
            amount: EXIT_AMOUNT,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &redeem_request])
        .expect("redemption request should succeed");

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &redeem_request.pubkey());
    ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault)
        .request(redeem_request.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault_token_account(reserve)
        .pending_vault(pending_vault)
        .asset_token_program(token::ID)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("approve redemption request should succeed");

    shutdown(&mut svm, &authority, share_mint.pubkey(), vault).expect("shutdown should succeed");

    ClaimBuilder::new()
        .user(user.pubkey())
        .owner(user.pubkey())
        .vault(vault)
        .request(redeem_request.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .pending_vault(Some(pending_vault))
        .user_share_account(None)
        .user_asset_account(Some(user_asset_account))
        .asset_token_program(token::ID)
        .share_token_program(None)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("claimable redemption should remain claimable during shutdown");
}

#[test]
fn shutdown_requires_redemptions_to_be_unpaused() {
    let (
        mut svm,
        authority,
        _mint_authority,
        _asset_mint,
        share_mint,
        _user,
        _reserve,
        _pending_vault,
        vault,
        _user_asset_account,
        _user_share_account,
    ) = setup(token::ID, token::ID, Some(true), false);

    let err = shutdown(&mut svm, &authority, share_mint.pubkey(), vault).unwrap_err();
    assert_error_code(&err, REDEMPTIONS_PAUSED, "RedemptionsPaused");
}

#[test]
fn shutdown_freezes_global_and_extension_configuration() {
    let (
        mut svm,
        authority,
        _mint_authority,
        _asset_mint,
        share_mint,
        _user,
        _reserve,
        _pending_vault,
        vault,
        _user_asset_account,
        _user_share_account,
    ) = setup(token::ID, token::ID, Some(false), false);

    shutdown(&mut svm, &authority, share_mint.pubkey(), vault).expect("shutdown should succeed");

    let global_pause_err = UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault)
        .paused(true)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&global_pause_err, VAULT_IS_CLOSING, "VaultIsClosing");

    let redemption_pause_err = UpdatePausableRedemptionsBuilder::new()
        .authority(authority.pubkey())
        .vault(vault)
        .paused(true)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&redemption_pause_err, VAULT_IS_CLOSING, "VaultIsClosing");
}
