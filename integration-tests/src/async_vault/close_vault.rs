use anchor_spl::{
    associated_token::get_associated_token_address_with_program_id,
    token::{self, spl_token},
    token_2022::{
        self,
        spl_token_2022::{extension::StateWithExtensions, state::Mint as Token2022Mint},
    },
};
use async_vault_client::{
    lite::SendTransaction, sdk::program_id, CancelQueuedDepositRequestBuilder,
    CancelQueuedRedemptionRequestBuilder, CloseVaultBuilder, CreateDepositRequestBuilder,
    CreateRedeemRequestBuilder, InitializeRedemptionQueueBuilder,
    InitializeSubscriptionQueueBuilder, InitializeVaultBuilder as InitializeAsyncVaultBuilder,
    RequestArgs, ShutdownVaultBuilder,
};
use litesvm::LiteSVM;
use solana_sdk::{
    account::ReadableAccount, program_option::COption, program_pack::Pack, signature::Keypair,
    signer::Signer,
};
use test_case::test_case;

use crate::{
    async_helper_functions::{
        assert_error_code, create_ata, get_token_account_amount, helper_mint_to, set_share_balance,
        set_up_async_vault, set_vault_pending_async_requests, set_vault_total_asset_balance,
    },
    async_vault::constants::{
        REDEMPTION_QUEUE_NOT_DRAINED, SHARE_MINT_SUPPLY_MUST_BE_ZERO_BEFORE_CLOSING,
        SUBSCRIPTION_QUEUE_NOT_DRAINED, UNAUTHORIZED_SIGNER, VAULT_HAS_OUTSTANDING_ASSET_BALANCE,
        VAULT_HAS_PENDING_ASYNC_REQUESTS, VAULT_MUST_BE_SHUTDOWN_BEFORE_CLOSING,
    },
};

#[allow(clippy::type_complexity)]
fn setup_vault() -> (
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
    setup_vault_with_token_programs(token::ID, token::ID)
}

#[allow(clippy::type_complexity)]
fn setup_vault_with_token_programs(
    asset_token_program: solana_sdk::pubkey::Pubkey,
    share_token_program: solana_sdk::pubkey::Pubkey,
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
        1_000_000,
    );

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

fn initialize_vault(
    svm: &mut LiteSVM,
    authority: &Keypair,
    share_mint: solana_sdk::pubkey::Pubkey,
    vault: solana_sdk::pubkey::Pubkey,
) {
    InitializeAsyncVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint)
        .vault(vault)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("initialize vault should succeed");
}

fn shutdown_vault(
    svm: &mut LiteSVM,
    authority: &Keypair,
    share_mint: solana_sdk::pubkey::Pubkey,
    vault: solana_sdk::pubkey::Pubkey,
) {
    ShutdownVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint)
        .vault(vault)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("shutdown vault should succeed");
}

fn close_vault(
    svm: &mut LiteSVM,
    authority: &Keypair,
    asset_mint: solana_sdk::pubkey::Pubkey,
    share_mint: solana_sdk::pubkey::Pubkey,
    vault: solana_sdk::pubkey::Pubkey,
    reserve: solana_sdk::pubkey::Pubkey,
    pending_vault: solana_sdk::pubkey::Pubkey,
) -> litesvm::types::TransactionResult {
    close_vault_with_token_programs(
        svm,
        authority,
        asset_mint,
        share_mint,
        vault,
        reserve,
        pending_vault,
        token::ID,
        token::ID,
    )
}

#[allow(clippy::too_many_arguments)]
fn close_vault_with_token_programs(
    svm: &mut LiteSVM,
    authority: &Keypair,
    asset_mint: solana_sdk::pubkey::Pubkey,
    share_mint: solana_sdk::pubkey::Pubkey,
    vault: solana_sdk::pubkey::Pubkey,
    reserve: solana_sdk::pubkey::Pubkey,
    pending_vault: solana_sdk::pubkey::Pubkey,
    asset_token_program: solana_sdk::pubkey::Pubkey,
    share_token_program: solana_sdk::pubkey::Pubkey,
) -> litesvm::types::TransactionResult {
    let authority_asset_token_account = get_associated_token_address_with_program_id(
        &authority.pubkey(),
        &asset_mint,
        &asset_token_program,
    );

    CloseVaultBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint)
        .share_mint(share_mint)
        .vault(vault)
        .reserve(reserve)
        .pending_vault(pending_vault)
        .authority_asset_token_account(authority_asset_token_account)
        .asset_token_program(asset_token_program)
        .share_token_program(share_token_program)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
}

fn assert_mint_authority(
    svm: &LiteSVM,
    share_mint: solana_sdk::pubkey::Pubkey,
    share_token_program: solana_sdk::pubkey::Pubkey,
    authority: solana_sdk::pubkey::Pubkey,
) {
    let mint_account = svm.get_account(&share_mint).unwrap();
    let mint_authority = if share_token_program == token_2022::ID {
        StateWithExtensions::<Token2022Mint>::unpack(mint_account.data())
            .expect("share mint should remain a valid Token-2022 mint")
            .base
            .mint_authority
    } else {
        spl_token::state::Mint::unpack(mint_account.data())
            .expect("share mint should remain a valid SPL mint")
            .mint_authority
    };
    assert_eq!(mint_authority, COption::Some(authority));
}

#[test_case(token::ID, token::ID ; "SPL Token asset and share mint")]
#[test_case(token_2022::ID, token_2022::ID ; "Token-2022 asset and share mint")]
#[test_case(token::ID, token_2022::ID ; "SPL Token asset and Token-2022 share mint")]
#[test_case(token_2022::ID, token::ID ; "Token-2022 asset and SPL Token share mint")]
fn close_vault_closes_all_vault_accounts_and_returns_share_mint_authority(
    asset_token_program: solana_sdk::pubkey::Pubkey,
    share_token_program: solana_sdk::pubkey::Pubkey,
) {
    let (
        mut svm,
        authority,
        _mint_authority,
        asset_mint,
        share_mint,
        _user,
        reserve,
        pending_vault,
        vault,
        _user_asset_account,
        _user_share_account,
    ) = setup_vault_with_token_programs(asset_token_program, share_token_program);

    initialize_vault(&mut svm, &authority, share_mint.pubkey(), vault);
    shutdown_vault(&mut svm, &authority, share_mint.pubkey(), vault);

    close_vault_with_token_programs(
        &mut svm,
        &authority,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault,
        reserve,
        pending_vault,
        asset_token_program,
        share_token_program,
    )
    .expect("close vault should succeed");

    assert!(svm.get_account(&vault).is_none());
    assert!(svm.get_account(&reserve).is_none());
    assert!(svm.get_account(&pending_vault).is_none());

    assert_mint_authority(
        &svm,
        share_mint.pubkey(),
        share_token_program,
        authority.pubkey(),
    );
}

#[test_case(false, 0, 0, 0, VAULT_MUST_BE_SHUTDOWN_BEFORE_CLOSING ; "vault must be shut down first")]
#[test_case(true, 1, 0, 0, VAULT_HAS_PENDING_ASYNC_REQUESTS ; "pending request blocks close")]
#[test_case(true, 0, 1, 0, VAULT_HAS_OUTSTANDING_ASSET_BALANCE ; "outstanding asset balance blocks close")]
#[test_case(true, 0, 0, 1, SHARE_MINT_SUPPLY_MUST_BE_ZERO_BEFORE_CLOSING ; "share supply blocks close")]
fn close_vault_rejects_unsatisfied_core_invariants(
    shutdown: bool,
    pending_requests: u16,
    total_asset_balance: u64,
    share_supply: u64,
    expected_error: u32,
) {
    let (
        mut svm,
        authority,
        _mint_authority,
        asset_mint,
        share_mint,
        _user,
        reserve,
        pending_vault,
        vault,
        _user_asset_account,
        user_share_account,
    ) = setup_vault();

    initialize_vault(&mut svm, &authority, share_mint.pubkey(), vault);
    if shutdown {
        shutdown_vault(&mut svm, &authority, share_mint.pubkey(), vault);
    }
    set_vault_pending_async_requests(&mut svm, vault, pending_requests);
    set_vault_total_asset_balance(&mut svm, vault, total_asset_balance);
    if share_supply > 0 {
        set_share_balance(
            &mut svm,
            &user_share_account,
            &share_mint.pubkey(),
            share_supply,
        );
    }
    let err = close_vault(
        &mut svm,
        &authority,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault,
        reserve,
        pending_vault,
    )
    .unwrap_err();
    assert_error_code(&err, expected_error, "");

    assert!(!svm.get_account(&vault).unwrap().data().is_empty());
    assert!(!svm.get_account(&reserve).unwrap().data().is_empty());
    assert!(!svm.get_account(&pending_vault).unwrap().data().is_empty());
}

#[test_case(token::ID, token::ID ; "SPL Token asset mint")]
#[test_case(token_2022::ID, token_2022::ID ; "Token-2022 asset mint")]
fn close_vault_sweeps_unaccounted_asset_dust_to_authority(
    asset_token_program: solana_sdk::pubkey::Pubkey,
    share_token_program: solana_sdk::pubkey::Pubkey,
) {
    let (
        mut svm,
        authority,
        mint_authority,
        asset_mint,
        share_mint,
        _user,
        reserve,
        pending_vault,
        vault,
        _user_asset_account,
        _user_share_account,
    ) = setup_vault_with_token_programs(asset_token_program, share_token_program);

    initialize_vault(&mut svm, &authority, share_mint.pubkey(), vault);
    shutdown_vault(&mut svm, &authority, share_mint.pubkey(), vault);

    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve,
        &mint_authority,
        1,
        &asset_token_program,
    );
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &pending_vault,
        &mint_authority,
        2,
        &asset_token_program,
    );

    let authority_asset_token_account = get_associated_token_address_with_program_id(
        &authority.pubkey(),
        &asset_mint.pubkey(),
        &asset_token_program,
    );
    let authority_balance_before = get_token_account_amount(
        &svm.get_account(&authority_asset_token_account)
            .expect("authority asset account should exist"),
    );

    close_vault_with_token_programs(
        &mut svm,
        &authority,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault,
        reserve,
        pending_vault,
        asset_token_program,
        share_token_program,
    )
    .expect("close vault should sweep unaccounted dust before closing token accounts");

    let authority_balance_after = get_token_account_amount(
        &svm.get_account(&authority_asset_token_account)
            .expect("authority asset account should remain"),
    );
    assert_eq!(authority_balance_after, authority_balance_before + 3);
    assert!(svm.get_account(&vault).is_none());
    assert!(svm.get_account(&reserve).is_none());
    assert!(svm.get_account(&pending_vault).is_none());
}

#[test]
fn close_vault_rejects_an_unauthorized_authority() {
    let (
        mut svm,
        authority,
        _mint_authority,
        asset_mint,
        share_mint,
        _user,
        reserve,
        pending_vault,
        vault,
        _user_asset_account,
        _user_share_account,
    ) = setup_vault();
    initialize_vault(&mut svm, &authority, share_mint.pubkey(), vault);
    shutdown_vault(&mut svm, &authority, share_mint.pubkey(), vault);

    let unauthorized = Keypair::new();
    svm.airdrop(&unauthorized.pubkey(), 1_000_000_000).unwrap();
    create_ata(&mut svm, &unauthorized, &asset_mint.pubkey(), &token::ID);
    let err = close_vault(
        &mut svm,
        &unauthorized,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault,
        reserve,
        pending_vault,
    )
    .unwrap_err();

    assert_error_code(&err, UNAUTHORIZED_SIGNER, "UnauthorizedSigner");
}

#[test_case(true, SUBSCRIPTION_QUEUE_NOT_DRAINED ; "subscription queue tombstone blocks close")]
#[test_case(false, REDEMPTION_QUEUE_NOT_DRAINED ; "redemption queue tombstone blocks close")]
fn close_vault_rejects_unprocessed_queue_tombstones(subscription_queue: bool, expected_error: u32) {
    let (
        mut svm,
        authority,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve,
        pending_vault,
        vault,
        user_asset_account,
        user_share_account,
    ) = setup_vault();

    if subscription_queue {
        InitializeSubscriptionQueueBuilder::new()
            .payer(authority.pubkey())
            .authority(authority.pubkey())
            .vault(vault)
            .instruction()
            .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
            .expect("initialize subscription queue should succeed");
    } else {
        InitializeRedemptionQueueBuilder::new()
            .payer(authority.pubkey())
            .authority(authority.pubkey())
            .vault(vault)
            .instruction()
            .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
            .expect("initialize redemption queue should succeed");
        set_share_balance(&mut svm, &user_share_account, &share_mint.pubkey(), 1);
    }

    initialize_vault(&mut svm, &authority, share_mint.pubkey(), vault);

    let request = Keypair::new();
    if subscription_queue {
        CreateDepositRequestBuilder::new()
            .user(user.pubkey())
            .asset_mint(asset_mint.pubkey())
            .share_mint(share_mint.pubkey())
            .request(request.pubkey())
            .vault(vault)
            .user_token_account(user_asset_account)
            .pending_vault(pending_vault)
            .asset_token_program(token::ID)
            .args(RequestArgs {
                amount: 1,
                operator: None,
            })
            .instruction()
            .send_transaction(&mut svm, &user.pubkey(), &[&user, &request])
            .expect("create queued deposit request should succeed");
        CancelQueuedDepositRequestBuilder::new()
            .user(user.pubkey())
            .asset_mint(asset_mint.pubkey())
            .share_mint(share_mint.pubkey())
            .vault(vault)
            .request(request.pubkey())
            .user_token_account(user_asset_account)
            .asset_pending_vault(pending_vault)
            .asset_token_program(token::ID)
            .instruction()
            .send_transaction(&mut svm, &user.pubkey(), &[&user])
            .expect("cancel queued deposit request should succeed");
    } else {
        CreateRedeemRequestBuilder::new()
            .user(user.pubkey())
            .asset_mint(asset_mint.pubkey())
            .share_mint(share_mint.pubkey())
            .request(request.pubkey())
            .vault(vault)
            .user_share_account(user_share_account)
            .share_token_program(token::ID)
            .args(RequestArgs {
                amount: 1,
                operator: None,
            })
            .instruction()
            .send_transaction(&mut svm, &user.pubkey(), &[&user, &request])
            .expect("create queued redemption request should succeed");
        CancelQueuedRedemptionRequestBuilder::new()
            .user(user.pubkey())
            .asset_mint(asset_mint.pubkey())
            .share_mint(share_mint.pubkey())
            .vault(vault)
            .request(request.pubkey())
            .user_share_account(user_share_account)
            .share_token_program(token::ID)
            .instruction()
            .send_transaction(&mut svm, &user.pubkey(), &[&user])
            .expect("cancel queued redemption request should succeed");
        set_share_balance(&mut svm, &user_share_account, &share_mint.pubkey(), 0);
    }

    shutdown_vault(&mut svm, &authority, share_mint.pubkey(), vault);
    let err = close_vault(
        &mut svm,
        &authority,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault,
        reserve,
        pending_vault,
    )
    .unwrap_err();
    assert_error_code(&err, expected_error, "");
}
