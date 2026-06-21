use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    self, Mint, MintTo, TokenAccount, TokenInterface, TransferChecked,
};

use crate::{
    error::AsyncVaultError,
    state::{Request, RequestState, RequestType, Vault, VAULT_CONFIG_SEED},
};

#[derive(Accounts)]
pub struct Claim<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    /// CHECK: handled in request constraint
    /// Necessary to send Request owner rent if Operator is claiming.
    #[account(mut)]
    pub owner: UncheckedAccount<'info>,

    pub asset_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        seeds = [VAULT_CONFIG_SEED, share_mint.key().as_ref()],
        bump = vault.bump,
        has_one = asset_mint @ AsyncVaultError::InvalidAssetMint,
        has_one = share_mint @ AsyncVaultError::InvalidShareMint,
    )]
    pub vault: Box<Account<'info, Vault>>,

    // Owner|Operator check in handler.
    #[account(
        mut,
        close = owner,
        has_one = owner @ AsyncVaultError::InvalidRequest,
        has_one = vault @ AsyncVaultError::InvalidRequest,
    )]
    pub request: Box<Account<'info, Request>>,

    /// Pending deposit vault — source for Redeem claims
    #[account(
        mut,
        constraint = vault.pending_vault == pending_vault.key() @ AsyncVaultError::InvalidPendingVault,
        token::mint = asset_mint,
        token::authority = vault,
        token::token_program = asset_token_program,
    )]
    pub pending_vault: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    /// User's share TokenAccount — receives minted shares on Deposit (must be owned by
    /// request.owner)
    #[account(
        mut,
        token::mint = share_mint,
        token::authority = request.owner,
    )]
    pub user_share_account: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    /// User's asset TokenAccount — receives transferred assets on Redeem (must be owned by
    /// request.owner)
    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = request.owner,
        token::token_program = asset_token_program,
    )]
    pub user_asset_account: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    pub asset_token_program: Interface<'info, TokenInterface>,

    /// Token program for share mint operations — only required for Deposit
    pub share_token_program: Option<Interface<'info, TokenInterface>>,
}

impl<'info> Claim<'info> {
    /// Mints pre-computed `shares` to the user's share account.
    pub fn deposit(&self, seeds: &[&[&[u8]]], shares: u64) -> Result<()> {
        let user_share_account = self
            .user_share_account
            .as_ref()
            .ok_or(AsyncVaultError::MissingRequiredAccount)?;
        let share_token_program = self
            .share_token_program
            .as_ref()
            .ok_or(AsyncVaultError::MissingRequiredAccount)?;

        token_interface::mint_to(
            CpiContext::new_with_signer(
                share_token_program.key(),
                MintTo {
                    mint: self.share_mint.to_account_info(),
                    to: user_share_account.to_account_info(),
                    authority: self.vault.to_account_info(),
                },
                seeds,
            ),
            shares,
        )
    }

    /// Transfers pre-computed `assets` from the pending vault to the user's asset account.
    pub fn redeem(&self, seeds: &[&[&[u8]]], assets: u64) -> Result<()> {
        let pending_vault = self
            .pending_vault
            .as_ref()
            .ok_or(AsyncVaultError::MissingRequiredAccount)?;
        let user_asset_account = self
            .user_asset_account
            .as_ref()
            .ok_or(AsyncVaultError::MissingRequiredAccount)?;

        token_interface::transfer_checked(
            CpiContext::new_with_signer(
                self.asset_token_program.key(),
                TransferChecked {
                    from: pending_vault.to_account_info(),
                    mint: self.asset_mint.to_account_info(),
                    to: user_asset_account.to_account_info(),
                    authority: self.vault.to_account_info(),
                },
                seeds,
            ),
            assets,
            self.asset_mint.decimals,
        )
    }
}

pub fn handler(ctx: Context<Claim>) -> Result<()> {
    ctx.accounts.vault.assert_unpaused_and_initialized()?;

    let request = &ctx.accounts.request;
    let user_key = ctx.accounts.user.key();

    require!(
        user_key == request.owner || request.operator.map_or(false, |op| user_key == op),
        AsyncVaultError::UnauthorizedSigner
    );

    require!(
        matches!(request.request_state, RequestState::Claimable),
        AsyncVaultError::RequestNotClaimable
    );

    let amount = request.amount;
    let share_mint_key = ctx.accounts.share_mint.key();
    let vault_bump = ctx.accounts.vault.bump;
    let seeds: &[&[&[u8]]] = &[&[VAULT_CONFIG_SEED, share_mint_key.as_ref(), &[vault_bump]]];

    // Mint/Transfer shares/assets to user
    match request.request_type {
        RequestType::Deposit => {
            ctx.accounts.deposit(seeds, amount)?;
        }
        RequestType::Redeem => {
            ctx.accounts.redeem(seeds, amount)?;
        }
    }

    ctx.accounts.vault.pending_async_requests = ctx
        .accounts
        .vault
        .pending_async_requests
        .checked_sub(1)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    Ok(())
}
