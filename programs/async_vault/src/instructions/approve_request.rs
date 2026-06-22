use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};

use crate::{
    error::AsyncVaultError,
    extensions::{
        fee::processor::{get_deposit_fee_and_net, get_withdrawal_fee_and_net},
        fifo_queue::check_and_advance_queue,
        redemption_queue::processor::{RedemptionQueue, RedemptionQueueRequest},
        subscription_queue::processor::{SubscriptionQueue, SubscriptionQueueRequest},
    },
    state::{Request, RequestState, RequestType, Vault, VAULT_CONFIG_SEED},
    utils::{
        calculate_assets, calculate_shares, validate_asset_mint_extensions_from_acct_info,
        validate_token_account_owner,
    },
};

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct ApproveRequestArgs {
    pub owner: Pubkey,
    pub request_type: RequestType,
    pub amount: u64,
    pub created_at: i64,
    pub nav_update_version: u64,
}

#[derive(Accounts)]
pub struct ApproveRequest<'info> {
    pub authority: Signer<'info>,

    pub asset_mint: InterfaceAccount<'info, Mint>,
    pub share_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        seeds = [VAULT_CONFIG_SEED, share_mint.key().as_ref()],
        bump = vault.bump,
        constraint = authority.key() == vault.authority @ AsyncVaultError::UnauthorizedSigner,
        has_one = asset_mint @ AsyncVaultError::InvalidAssetMint,
        has_one = share_mint @ AsyncVaultError::InvalidShareMint,
    )]
    pub vault: Box<Account<'info, Vault>>,

    #[account(
        mut,
        has_one = vault @ AsyncVaultError::InvalidRequest,
    )]
    pub request: Box<Account<'info, Request>>,

    #[account(
        mut,
        constraint = vault.vault_token_account == vault_token_account.key(),
        token::mint = asset_mint,
        token::authority = vault,
        token::token_program = asset_token_program,
    )]
    pub vault_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        constraint = vault.pending_vault == pending_vault.key() @ AsyncVaultError::InvalidPendingVault,
        token::mint = asset_mint,
        token::authority = vault,
        token::token_program = asset_token_program,
    )]
    pub pending_vault: InterfaceAccount<'info, TokenAccount>,

    pub asset_token_program: Interface<'info, TokenInterface>,
}

// TODO [SYSTEM DESIGN]: As this is currently written, the fee_recipient_token_account is only
// required if the DepositFee|WithrawFee Extension is enabled AND produces a fee > 0.
// This creates an inconsistent API at the expense of a very minor optimization.

impl<'info> ApproveRequest<'info> {
    /// Transfers assets from the pending vault (aka escrow) to the supplied
    /// TokenAccount.
    fn transfer_asset_from_pending_vault(
        &self,
        to: AccountInfo<'info>,
        amount: u64,
        seeds: &[&[&[u8]]],
    ) -> Result<()> {
        token_interface::transfer_checked(
            CpiContext::new_with_signer(
                self.asset_token_program.key(),
                TransferChecked {
                    from: self.pending_vault.to_account_info(),
                    mint: self.asset_mint.to_account_info(),
                    to,
                    authority: self.vault.to_account_info(),
                },
                seeds,
            ),
            amount,
            self.asset_mint.decimals,
        )
    }

    /// Transfers assets from the vault to the supplied TokenAccount.
    fn transfer_asset_from_vault(
        &self,
        to: AccountInfo<'info>,
        amount: u64,
        seeds: &[&[&[u8]]],
    ) -> Result<()> {
        token_interface::transfer_checked(
            CpiContext::new_with_signer(
                self.asset_token_program.key(),
                TransferChecked {
                    from: self.vault_token_account.to_account_info(),
                    mint: self.asset_mint.to_account_info(),
                    to,
                    authority: self.vault.to_account_info(),
                },
                seeds,
            ),
            amount,
            self.asset_mint.decimals,
        )
    }

    /// Transfers assets from pending vault to vault, enabling the Authority to withdraw
    /// in a future transaction.
    fn settle_deposit(&self, seeds: &[&[&[u8]]], amount: u64) -> Result<()> {
        self.transfer_asset_from_pending_vault(
            self.vault_token_account.to_account_info(),
            amount,
            seeds,
        )
    }

    /// Transfers assets from vault to pending vault, removing them from the supply
    /// that the Authority may withdraw from.
    fn settle_redeem(&self, seeds: &[&[&[u8]]], assets: u64) -> Result<()> {
        self.transfer_asset_from_vault(self.pending_vault.to_account_info(), assets, seeds)
    }
}

pub fn handler<'info>(
    ctx: Context<'info, ApproveRequest<'info>>,
    args: ApproveRequestArgs,
) -> Result<()> {
    ctx.accounts.vault.assert_unpaused_and_initialized()?;

    validate_asset_mint_extensions_from_acct_info(&ctx.accounts.asset_mint.to_account_info())?;

    require!(
        matches!(ctx.accounts.request.request_state, RequestState::Pending),
        AsyncVaultError::RequestNotPending
    );

    let request = &ctx.accounts.request;
    require!(
        request.owner == args.owner
            && request.request_type == args.request_type
            && request.amount == args.amount
            && request.created_at == args.created_at
            && request.nav_update_version == args.nav_update_version,
        AsyncVaultError::ApprovalRequestMismatch
    );

    require!(ctx.accounts.vault.nav > 0, AsyncVaultError::NavIsNotSet);

    let nav = ctx.accounts.vault.nav;
    let decimals = ctx.accounts.share_mint.decimals;
    let share_mint_key = ctx.accounts.share_mint.key();
    let vault_bump = ctx.accounts.vault.bump;
    let seeds: &[&[&[u8]]] = &[&[VAULT_CONFIG_SEED, share_mint_key.as_ref(), &[vault_bump]]];

    let is_deposit = matches!(ctx.accounts.request.request_type, RequestType::Deposit);
    let original_amount = ctx.accounts.request.amount;

    // Extension: SubscriptionQueue — enforce FIFO ordering for deposit requests.
    if is_deposit {
        check_and_advance_queue::<SubscriptionQueue, SubscriptionQueueRequest>(
            &ctx.accounts.vault.to_account_info(),
            &ctx.accounts.request.to_account_info(),
        )?;
    }

    // Extension: RedemptionQueue — enforce FIFO ordering for redeem requests.
    if !is_deposit {
        check_and_advance_queue::<RedemptionQueue, RedemptionQueueRequest>(
            &ctx.accounts.vault.to_account_info(),
            &ctx.accounts.request.to_account_info(),
        )?;
    }

    let mut remaining = ctx.remaining_accounts.iter();

    // Transfer assets between Vault and Pending Vault (aka escrow)
    let (claimable_amount, balance_delta) = if is_deposit {
        // Check for DepositFee Extension and calculate fee owed
        let (deposit_fee, net_deposit) =
            get_deposit_fee_and_net(&ctx.accounts.vault.to_account_info(), original_amount)?;
        require!(net_deposit > 0, AsyncVaultError::InsufficientDepositAmount);
        // Shares to be minted, floored (protocol favorable)
        let shares = calculate_shares(nav, decimals, net_deposit)?;
        require!(shares > 0, AsyncVaultError::InsufficientDepositAmount);
        if deposit_fee > 0 {
            // Validate and transfer fees to fee_recipient
            let fee_recipient_token_account_info = remaining
                .next()
                .ok_or(AsyncVaultError::MissingFeeRecipient)?;
            validate_token_account_owner(
                fee_recipient_token_account_info,
                &ctx.accounts.vault.fee_recipient,
            )?;
            ctx.accounts.transfer_asset_from_pending_vault(
                fee_recipient_token_account_info.to_account_info(),
                deposit_fee,
                seeds,
            )?;
        }
        ctx.accounts.settle_deposit(seeds, net_deposit)?;
        (shares, net_deposit)
    } else {
        // Assets to be transferred, floored (protocol favorable)
        let assets = calculate_assets(nav, decimals, original_amount)?;

        // Check for WithdrawFee Extension and calculate fee owed
        let (withdraw_fee, net_assets) =
            get_withdrawal_fee_and_net(&ctx.accounts.vault.to_account_info(), assets)?;
        if withdraw_fee > 0 {
            // Validate and transfer fees to fee_recipient
            let fee_recipient_token_account_info = remaining
                .next()
                .ok_or(AsyncVaultError::MissingFeeRecipient)?;
            validate_token_account_owner(
                fee_recipient_token_account_info,
                &ctx.accounts.vault.fee_recipient,
            )?;
            ctx.accounts.transfer_asset_from_vault(
                fee_recipient_token_account_info.to_account_info(),
                withdraw_fee,
                seeds,
            )?;
        }
        ctx.accounts.settle_redeem(seeds, net_assets)?;
        (net_assets, assets)
    };

    let vault = &mut ctx.accounts.vault;
    let request = &mut ctx.accounts.request;

    // Update Vault's `total_asset_balance`
    if is_deposit {
        vault.total_asset_balance = vault
            .total_asset_balance
            .checked_add(balance_delta)
            .ok_or(AsyncVaultError::ArithmeticError)?;
    } else {
        vault.total_asset_balance = vault
            .total_asset_balance
            .checked_sub(balance_delta)
            .ok_or(AsyncVaultError::ArithmeticError)?;
    }

    // Update Request's amount with the claimable amount
    request.amount = claimable_amount;
    request.price = nav;
    request.request_state = RequestState::Claimable;

    Ok(())
}
