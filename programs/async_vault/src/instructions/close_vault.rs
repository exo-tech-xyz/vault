use anchor_lang::prelude::*;
use anchor_spl::{
    token_2022::{set_authority, spl_token_2022::instruction::AuthorityType, SetAuthority},
    token_interface::{
        self, close_account, CloseAccount, Mint, TokenAccount, TokenInterface, TransferChecked,
    },
};

use crate::{
    error::AsyncVaultError,
    extensions::{
        read_vault_extension, redemption_queue::processor::RedemptionQueue,
        subscription_queue::processor::SubscriptionQueue, FifoQueue,
    },
    state::{Vault, VAULT_CONFIG_SEED},
};

#[derive(Accounts)]
pub struct CloseVault<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    pub asset_mint: InterfaceAccount<'info, Mint>,

    #[account(mut)]
    pub share_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        constraint = authority.key() == vault.authority @ AsyncVaultError::UnauthorizedSigner,
        seeds = [VAULT_CONFIG_SEED, share_mint.key().as_ref()],
        bump = vault.bump,
        close = authority,
    )]
    pub vault: Box<Account<'info, Vault>>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = vault,
        token::token_program = asset_token_program,
        constraint = vault.vault_token_account == reserve.key(),
    )]
    pub reserve: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = vault,
        token::token_program = asset_token_program,
        constraint = vault.pending_vault == pending_vault.key() @ AsyncVaultError::InvalidPendingVault,
    )]
    pub pending_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = authority,
        token::token_program = asset_token_program,
    )]
    pub authority_asset_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    pub asset_token_program: Interface<'info, TokenInterface>,
    pub share_token_program: Interface<'info, TokenInterface>,
}

impl<'info> CloseVault<'info> {
    fn assert_queue_drained<Q: FifoQueue>(&self, error: AsyncVaultError) -> Result<()> {
        let vault_info = self.vault.to_account_info();
        let vault_data = vault_info
            .data
            .try_borrow()
            .map_err(|_| ProgramError::AccountBorrowFailed)?;
        if let Some(queue) = read_vault_extension::<Q>(&vault_data)? {
            if queue.last_processed() != queue.total() {
                return Err(error.into());
            }
        }
        Ok(())
    }

    fn close_asset_account(&self, account: AccountInfo<'info>) -> Result<()> {
        let share_mint_key = self.share_mint.key();
        let vault_bump = self.vault.bump;
        let signer_seeds: &[&[&[u8]]] =
            &[&[VAULT_CONFIG_SEED, share_mint_key.as_ref(), &[vault_bump]]];
        close_account(CpiContext::new_with_signer(
            self.asset_token_program.key(),
            CloseAccount {
                account,
                destination: self.authority.to_account_info(),
                authority: self.vault.to_account_info(),
            },
            signer_seeds,
        ))
    }

    fn sweep_asset_account(&self, account: AccountInfo<'info>, amount: u64) -> Result<()> {
        if amount == 0 {
            return Ok(());
        }

        let share_mint_key = self.share_mint.key();
        let vault_bump = self.vault.bump;
        let signer_seeds: &[&[&[u8]]] =
            &[&[VAULT_CONFIG_SEED, share_mint_key.as_ref(), &[vault_bump]]];
        token_interface::transfer_checked(
            CpiContext::new_with_signer(
                self.asset_token_program.key(),
                TransferChecked {
                    from: account,
                    mint: self.asset_mint.to_account_info(),
                    to: self.authority_asset_token_account.to_account_info(),
                    authority: self.vault.to_account_info(),
                },
                signer_seeds,
            ),
            amount,
            self.asset_mint.decimals,
        )
    }

    fn return_share_mint_authority(&self) -> Result<()> {
        let share_mint_key = self.share_mint.key();
        let vault_bump = self.vault.bump;
        let signer_seeds: &[&[&[u8]]] =
            &[&[VAULT_CONFIG_SEED, share_mint_key.as_ref(), &[vault_bump]]];
        set_authority(
            CpiContext::new_with_signer(
                self.share_token_program.key(),
                SetAuthority {
                    current_authority: self.vault.to_account_info(),
                    account_or_mint: self.share_mint.to_account_info(),
                },
                signer_seeds,
            ),
            AuthorityType::MintTokens,
            Some(self.authority.key()),
        )
    }
}

pub fn handler(ctx: Context<CloseVault>) -> Result<()> {
    let accounts = &ctx.accounts;

    require!(
        accounts.vault.pending_async_requests == 0,
        AsyncVaultError::VaultHasPendingAsyncRequests
    );
    require!(
        accounts.share_mint.supply == 0,
        AsyncVaultError::ShareMintSupplyMustBeZeroBeforeClosing
    );
    accounts
        .assert_queue_drained::<SubscriptionQueue>(AsyncVaultError::SubscriptionQueueNotDrained)?;
    accounts.assert_queue_drained::<RedemptionQueue>(AsyncVaultError::RedemptionQueueNotDrained)?;

    accounts.sweep_asset_account(accounts.reserve.to_account_info(), accounts.reserve.amount)?;
    accounts.sweep_asset_account(
        accounts.pending_vault.to_account_info(),
        accounts.pending_vault.amount,
    )?;
    accounts.close_asset_account(accounts.reserve.to_account_info())?;
    accounts.close_asset_account(accounts.pending_vault.to_account_info())?;
    accounts.return_share_mint_authority()
}
