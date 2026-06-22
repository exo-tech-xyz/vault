use anchor_lang::prelude::*;
use anchor_spl::{
    token_2022::{set_authority, spl_token_2022::instruction::AuthorityType, SetAuthority},
    token_interface::{Mint, TokenAccount, TokenInterface},
};

use crate::{
    error::AsyncVaultError,
    state::{Vault, PENDING_VAULT_SEED, RESERVE_CONFIG_SEED, VAULT_CONFIG_SEED},
    utils::{
        validate_asset_mint_extensions_from_acct_info,
        validate_share_mint_extensions_from_acct_info,
    },
};

#[derive(AnchorDeserialize, AnchorSerialize)]
pub struct AsyncVaultArgs {
    authority: Pubkey,
    fee_recipient: Pubkey,
}

#[derive(Accounts)]
pub struct CreateVault<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub mint_authority: Signer<'info>,

    pub asset_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        constraint = share_mint.key() != asset_mint.key() @ AsyncVaultError::MintsShouldBeDifferent,
    )]
    pub share_mint: InterfaceAccount<'info, Mint>,

    #[account(
        init,
        space = 8 + Vault::INIT_SPACE,
        payer = payer,
        seeds = [VAULT_CONFIG_SEED, share_mint.key().as_ref()],
        bump
    )]
    pub vault: Account<'info, Vault>,

    #[account(
        init,
        token::authority = vault,
        token::mint = asset_mint,
        token::token_program = asset_token_program,
        payer = payer,
        seeds = [RESERVE_CONFIG_SEED, share_mint.key().as_ref()],
        bump,
    )]
    pub reserve: InterfaceAccount<'info, TokenAccount>,

    #[account(
        init,
        token::authority = vault,
        token::mint = asset_mint,
        token::token_program = asset_token_program,
        payer = payer,
        seeds = [PENDING_VAULT_SEED, share_mint.key().as_ref()],
        bump,
    )]
    pub pending_vault: InterfaceAccount<'info, TokenAccount>,

    pub asset_token_program: Interface<'info, TokenInterface>,
    pub share_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> CreateVault<'info> {
    pub fn set_new_authority(&mut self, new_authority: Pubkey) -> Result<()> {
        let cpi_accounts = SetAuthority {
            current_authority: self.mint_authority.to_account_info(),
            account_or_mint: self.share_mint.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(self.share_token_program.key(), cpi_accounts);
        set_authority(cpi_ctx, AuthorityType::MintTokens, Some(new_authority))
    }
}

/// Creates a new async vault with the given configuration.
///
/// Initializes three PDAs seeded by the share mint:
/// - **vault**: the config account holding all async-vault state
/// - **reserve**: token account for confirmed asset deposits
/// - **pending_vault**: token account for assets awaiting share issuance
///
/// Transfers the share mint authority to the vault PDA so only the
/// program can mint/burn share tokens.
///
/// The vault starts `initialized = false`; call
/// `initialize_vault` after configuring extensions to activate it.
/// Freeze authority is not transferred since is up to the implementator to manage it.
pub fn handler(ctx: Context<CreateVault>, args: AsyncVaultArgs) -> Result<()> {
    require!(
        ctx.accounts.share_mint.supply == 0,
        AsyncVaultError::ShareMintSupplyShouldBeZero
    );

    validate_asset_mint_extensions_from_acct_info(&ctx.accounts.asset_mint.to_account_info())?;
    validate_share_mint_extensions_from_acct_info(&ctx.accounts.share_mint.to_account_info())?;

    ctx.accounts.set_new_authority(ctx.accounts.vault.key())?;

    ctx.accounts.vault.set_inner(Vault {
        asset_mint: ctx.accounts.asset_mint.key(),
        share_mint: ctx.accounts.share_mint.key(),
        vault_token_account: ctx.accounts.reserve.key(),
        authority: args.authority,
        fee_recipient: args.fee_recipient,
        paused: false,
        initialized: false,
        pending_vault: ctx.accounts.pending_vault.key(),
        nav: 0,
        nav_version: 0,
        pending_async_requests: 0,
        total_asset_balance: 0,
        pending_authority: None,
        reserve_bump: ctx.bumps.reserve,
        pending_vault_bump: ctx.bumps.pending_vault,
        bump: ctx.bumps.vault,
    });

    Ok(())
}
