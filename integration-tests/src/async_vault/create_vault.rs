use anchor_spl::{
    token::{self, spl_token},
    token_2022::{self, spl_token_2022},
};
use async_vault_client::{
    lite::SendTransaction, sdk::program_id, CreateVaultBuilder as CreateAsyncVaultBuilder, Vault,
};
use litesvm::LiteSVM;
use solana_sdk::{
    account::ReadableAccount, program_pack::Pack, pubkey::Pubkey, signature::Keypair,
    signer::Signer, transaction::Transaction,
};
use solana_system_interface::instruction::create_account;
use test_case::test_case;

use crate::{
    async_helper_functions::{
        assert_error_code, create_mint, create_mint_with_confidential_mint_burn,
        create_mint_with_mint_close_authority, create_mint_with_transfer_fee, PENDING_VAULT_SEED,
        RESERVE_CONFIG_SEED, VAULT_CONFIG_SEED,
    },
    async_vault::constants::{
        INVALID_ASSET_MINT_EXTENSIONS, INVALID_SHARE_MINT_EXTENSIONS, MINTS_SHOULD_BE_DIFFERENT,
        SHARE_MINT_SUPPLY_SHOULD_BE_ZERO,
    },
};

#[test_case(true, false, token::ID,token::ID, 0 ; "Token program for both mints")]
#[test_case(true, false, token_2022::ID,token_2022::ID, 0 ; "Token 2022 program for both mints")]
#[test_case(true, false, token::ID,token_2022::ID, 0 ; "Token program for asset, Token program 2022 for share")]
#[test_case(true, false, token_2022::ID,token::ID, 0 ; "Token 2022 program for asset, Token program for share")]
#[test_case(false, false, token_2022::ID,token_2022::ID, 0 ; "invalid mint authority fails")]
#[test_case(true, true, token_2022::ID,token_2022::ID, 0 ; "same mints fails")]
#[test_case(true, false, token_2022::ID,token_2022::ID, 1 ; "nonzero transfer fee asset mint fails")]
fn test_create_vault(
    use_valid_mint_authority: bool,
    use_same_mints: bool,
    asset_program: Pubkey,
    share_program: Pubkey,
    asset_transfer_fee: u16,
) {
    let mut svm = LiteSVM::new();

    let program_bytes = include_bytes!("../../../target/deploy/async_vault.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let authority = Keypair::new();
    let payer = Keypair::new();
    let mint_authority = Keypair::new();
    let fake_mint_authority = Keypair::new();
    let asset_mint = Keypair::new();
    let share_mint = Keypair::new();
    let fee_recipient = Keypair::new();

    svm.airdrop(&authority.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&fee_recipient.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&mint_authority.pubkey(), 1_000_000_000)
        .unwrap();
    svm.airdrop(&fake_mint_authority.pubkey(), 1_000_000_000)
        .unwrap();

    if asset_program == token_2022::ID {
        create_mint_with_transfer_fee(
            &mut svm,
            &mint_authority,
            &asset_mint,
            asset_transfer_fee,
            u64::MAX,
        );
    } else {
        create_mint(&mut svm, &mint_authority, &asset_mint, &asset_program);
    }
    if !use_same_mints {
        create_mint(&mut svm, &mint_authority, &share_mint, &share_program);
    }

    let effective_share_mint = if use_same_mints {
        asset_mint.pubkey()
    } else {
        share_mint.pubkey()
    };

    let (reserve_pubkey, _) = Pubkey::find_program_address(
        &[RESERVE_CONFIG_SEED, effective_share_mint.as_ref()],
        &program_id(),
    );
    let (pending_vault_pubkey, _) = Pubkey::find_program_address(
        &[PENDING_VAULT_SEED, effective_share_mint.as_ref()],
        &program_id(),
    );

    let (vault_pubkey, _) = Pubkey::find_program_address(
        &[VAULT_CONFIG_SEED, effective_share_mint.as_ref()],
        &program_id(),
    );

    let effective_mint_authority = if use_valid_mint_authority {
        &mint_authority
    } else {
        &fake_mint_authority
    };

    let result = CreateAsyncVaultBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(effective_mint_authority.pubkey())
        .fee_recipient(fee_recipient.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(effective_share_mint)
        .reserve(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .vault(vault_pubkey)
        .asset_token_program(asset_program)
        .share_token_program(share_program)
        .authority(authority.pubkey())
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, effective_mint_authority],
        );

    let should_succeed = use_valid_mint_authority && !use_same_mints && asset_transfer_fee == 0;

    if should_succeed {
        result.expect("async vault creation should succeed");

        let vault_account = svm
            .get_account(&vault_pubkey)
            .expect("Vault account should exist");
        assert!(!vault_account.data.is_empty(), "Vault should have data");

        let vault_config = Vault::from_bytes(vault_account.data()).unwrap();
        assert_eq!(vault_config.authority, authority.pubkey());
        assert_eq!(vault_config.asset_mint, asset_mint.pubkey());
        assert_eq!(vault_config.share_mint, effective_share_mint);
        assert_eq!(vault_config.vault_token_account, reserve_pubkey);
        assert_eq!(vault_config.pending_vault, pending_vault_pubkey);
        assert_eq!(vault_config.paused, false);
        assert!(!vault_config.initialized);
        assert_eq!(vault_config.nav, 0);
        assert_eq!(vault_config.nav_version, 0);
        assert_eq!(vault_config.pending_async_requests, 0);
        assert_eq!(vault_config.total_asset_balance, 0);
    } else {
        let err_result = &result.unwrap_err();
        if !use_valid_mint_authority {
            assert_error_code(err_result, 4, "OwnerMismatch");
        }

        if use_same_mints {
            assert_error_code(
                err_result,
                MINTS_SHOULD_BE_DIFFERENT,
                "Mints should be different.",
            );
        }
        if asset_transfer_fee > 0 {
            assert_error_code(
                err_result,
                INVALID_ASSET_MINT_EXTENSIONS,
                "Asset mint has invalid extensions.",
            );
        }
    }
}

#[test]
fn test_create_vault_nonzero_share_mint_supply_fails() {
    let mut svm = LiteSVM::new();

    let program_bytes = include_bytes!("../../../target/deploy/async_vault.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let authority = Keypair::new();
    let payer = Keypair::new();
    let mint_authority = Keypair::new();
    let asset_mint = Keypair::new();
    let share_mint = Keypair::new();
    let token_account_kp = Keypair::new();
    let fee_recipient = Keypair::new();

    svm.airdrop(&authority.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&fee_recipient.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&mint_authority.pubkey(), 1_000_000_000)
        .unwrap();

    create_mint(&mut svm, &mint_authority, &asset_mint, &spl_token::ID);
    create_mint(&mut svm, &mint_authority, &share_mint, &spl_token::ID);

    let rent = svm.minimum_balance_for_rent_exemption(spl_token_2022::state::Account::LEN);
    let create_account_ix = create_account(
        &mint_authority.pubkey(),
        &token_account_kp.pubkey(),
        rent,
        spl_token_2022::state::Account::LEN as u64,
        &spl_token::id(),
    );
    let init_account_ix = spl_token_2022::instruction::initialize_account(
        &spl_token::ID,
        &token_account_kp.pubkey(),
        &share_mint.pubkey(),
        &mint_authority.pubkey(),
    )
    .unwrap();
    let mint_to_ix = spl_token_2022::instruction::mint_to(
        &spl_token::ID,
        &share_mint.pubkey(),
        &token_account_kp.pubkey(),
        &mint_authority.pubkey(),
        &[],
        1,
    )
    .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[create_account_ix, init_account_ix, mint_to_ix],
        Some(&mint_authority.pubkey()),
        &[&mint_authority, &token_account_kp],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx)
        .expect("token account creation and mint_to should succeed");

    let (reserve_pubkey, _) = Pubkey::find_program_address(
        &[RESERVE_CONFIG_SEED, share_mint.pubkey().as_ref()],
        &program_id(),
    );
    let (pending_vault_pubkey, _) = Pubkey::find_program_address(
        &[PENDING_VAULT_SEED, share_mint.pubkey().as_ref()],
        &program_id(),
    );

    let (vault_pubkey, _) = Pubkey::find_program_address(
        &[VAULT_CONFIG_SEED, share_mint.pubkey().as_ref()],
        &program_id(),
    );

    let result = CreateAsyncVaultBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .fee_recipient(fee_recipient.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .reserve(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .vault(vault_pubkey)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .authority(authority.pubkey())
        .instruction()
        .send_transaction(&mut svm, &payer.pubkey(), &[&payer, &mint_authority]);

    let err_result = &result.unwrap_err();
    assert_error_code(
        err_result,
        SHARE_MINT_SUPPLY_SHOULD_BE_ZERO,
        "Share mint supply should be zero.",
    );
}

#[test]
fn test_create_vault_confidential_mint_burn_share_mint_fails() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let authority = Keypair::new();
    let payer = Keypair::new();
    let mint_authority = Keypair::new();
    let asset_mint = Keypair::new();
    let share_mint = Keypair::new();
    let fee_recipient = Keypair::new();

    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&mint_authority.pubkey(), 1_000_000_000)
        .unwrap();

    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
    create_mint_with_confidential_mint_burn(&mut svm, &share_mint, 9);

    let (reserve_pubkey, _) = Pubkey::find_program_address(
        &[RESERVE_CONFIG_SEED, share_mint.pubkey().as_ref()],
        &program_id(),
    );
    let (pending_vault_pubkey, _) = Pubkey::find_program_address(
        &[PENDING_VAULT_SEED, share_mint.pubkey().as_ref()],
        &program_id(),
    );
    let (vault_pubkey, _) = Pubkey::find_program_address(
        &[VAULT_CONFIG_SEED, share_mint.pubkey().as_ref()],
        &program_id(),
    );

    let result = CreateAsyncVaultBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .fee_recipient(fee_recipient.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .reserve(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .vault(vault_pubkey)
        .asset_token_program(token::ID)
        .share_token_program(token_2022::ID)
        .authority(authority.pubkey())
        .instruction()
        .send_transaction(&mut svm, &payer.pubkey(), &[&payer, &mint_authority]);

    let err = result.unwrap_err();
    assert_error_code(
        &err,
        INVALID_SHARE_MINT_EXTENSIONS,
        "Share mint has invalid extensions.",
    );
}

#[test]
fn test_create_vault_mint_close_authority_share_mint_fails() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let authority = Keypair::new();
    let payer = Keypair::new();
    let mint_authority = Keypair::new();
    let asset_mint = Keypair::new();
    let share_mint = Keypair::new();
    let fee_recipient = Keypair::new();

    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&mint_authority.pubkey(), 1_000_000_000)
        .unwrap();

    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
    create_mint_with_mint_close_authority(&mut svm, &mint_authority, &share_mint);

    let (reserve_pubkey, _) = Pubkey::find_program_address(
        &[RESERVE_CONFIG_SEED, share_mint.pubkey().as_ref()],
        &program_id(),
    );
    let (pending_vault_pubkey, _) = Pubkey::find_program_address(
        &[PENDING_VAULT_SEED, share_mint.pubkey().as_ref()],
        &program_id(),
    );
    let (vault_pubkey, _) = Pubkey::find_program_address(
        &[VAULT_CONFIG_SEED, share_mint.pubkey().as_ref()],
        &program_id(),
    );

    let result = CreateAsyncVaultBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .fee_recipient(fee_recipient.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .reserve(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .vault(vault_pubkey)
        .asset_token_program(token::ID)
        .share_token_program(token_2022::ID)
        .authority(authority.pubkey())
        .instruction()
        .send_transaction(&mut svm, &payer.pubkey(), &[&payer, &mint_authority]);

    let err = result.unwrap_err();
    assert_error_code(
        &err,
        INVALID_SHARE_MINT_EXTENSIONS,
        "Share mint has invalid extensions.",
    );
}
