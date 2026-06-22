use crate::{
    error::AsyncVaultError,
    extensions::{
        self,
        fifo_queue::next_queue_request_id,
        request_extensions::{compute_request_extension_space, init_request_extension},
        subscription_queue::processor::{SubscriptionQueue, SubscriptionQueueRequest},
    },
    utils::validate_asset_mint_extensions_from_acct_info,
};
use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};

use crate::state::{Request, RequestState, RequestType, Vault, VAULT_CONFIG_SEED};

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct RequestArgs {
    pub amount: u64,
    pub operator: Option<Pubkey>,
}

#[derive(Accounts)]
pub struct CreateDepositRequest<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    pub asset_mint: InterfaceAccount<'info, Mint>,
    pub share_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        has_one = asset_mint @ AsyncVaultError::InvalidAssetMint,
        has_one = share_mint @ AsyncVaultError::InvalidShareMint,
        seeds = [VAULT_CONFIG_SEED, share_mint.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Box<Account<'info, Vault>>,

    // Space is extended conditionally: if SubscriptionQueue is active on the vault,
    // extra bytes are reserved for the SubscriptionQueueRequest TLV extension.
    #[account(
        init,
        space = 8 + Request::INIT_SPACE + compute_request_extension_space(&vault.to_account_info()),
        payer = user,
    )]
    pub request: Account<'info, Request>,

    #[account(
        mut,
        token::mint = asset_mint.key(),
        token::authority = user
    )]
    pub user_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        token::mint = asset_mint.key(),
        token::authority = vault,
        token::token_program = asset_token_program,
        constraint = vault.pending_vault.key() == pending_vault.key() @ AsyncVaultError::InvalidPendingVault
    )]
    pub pending_vault: InterfaceAccount<'info, TokenAccount>,

    pub asset_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> CreateDepositRequest<'info> {
    /// Transfers assets from the User's TokenAccount to the pending vault (aka escrow)
    pub fn transfer_assets_from_user_to_pending_vault(&self, amount: u64) -> Result<()> {
        let cpi_ctx = CpiContext::new(
            self.asset_token_program.key(),
            TransferChecked {
                from: self.user_token_account.to_account_info(),
                mint: self.asset_mint.to_account_info(),
                to: self.pending_vault.to_account_info(),
                authority: self.user.to_account_info(),
            },
        );
        token_interface::transfer_checked(cpi_ctx, amount, self.asset_mint.decimals)
    }
}

pub fn handler(ctx: Context<CreateDepositRequest>, args: RequestArgs) -> Result<()> {
    ctx.accounts.vault.assert_unpaused_and_initialized()?;

    extensions::pausable_subscriptions::check_subscriptions_paused(
        &ctx.accounts.vault.to_account_info(),
    )?;

    extensions::min_subscription::check_min_subscription_amount(
        &ctx.accounts.vault.to_account_info(),
        args.amount,
    )?;

    let deposit_fee = extensions::fee::processor::get_deposit_fee(
        &ctx.accounts.vault.to_account_info(),
        args.amount,
    )?;
    require!(
        args.amount > deposit_fee,
        AsyncVaultError::InsufficientDepositAmount
    );

    validate_asset_mint_extensions_from_acct_info(&ctx.accounts.asset_mint.to_account_info())?;

    // SAFETY: TransferFees are required to be 0, therefore using args.amount is safe.
    ctx.accounts
        .transfer_assets_from_user_to_pending_vault(args.amount)?;

    let current_timestamp = Clock::get()?.unix_timestamp;
    ctx.accounts.request.set_inner(Request {
        vault: ctx.accounts.vault.key(),
        request_type: RequestType::Deposit,
        request_state: RequestState::Pending,
        owner: ctx.accounts.user.key(),
        amount: args.amount,
        price: ctx.accounts.vault.nav,
        asset_mint_address: ctx.accounts.asset_mint.key(),
        created_at: current_timestamp,
        nav_update_version: ctx.accounts.vault.nav_version,
        operator: args.operator,
    });

    ctx.accounts.vault.pending_async_requests = ctx
        .accounts
        .vault
        .pending_async_requests
        .checked_add(1)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    // Extension: SubscriptionQueue — increment counter and tag the request with its ID.
    if let Some(id) =
        next_queue_request_id::<SubscriptionQueue>(&ctx.accounts.vault.to_account_info())?
    {
        init_request_extension(
            &ctx.accounts.request.to_account_info(),
            &SubscriptionQueueRequest { id },
        )?;
    }

    Ok(())
}
