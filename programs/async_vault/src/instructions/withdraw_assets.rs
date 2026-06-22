use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};

use crate::{
    error::AsyncVaultError,
    state::{Vault, VAULT_CONFIG_SEED},
};

#[derive(Accounts)]
pub struct WithdrawAssets<'info> {
    pub authority: Signer<'info>,

    pub asset_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        has_one = asset_mint @ AsyncVaultError::InvalidAssetMint,
        constraint = authority.key() == vault.authority @ AsyncVaultError::UnauthorizedSigner,
    )]
    pub vault: Account<'info, Vault>,

    #[account(
        mut,
        token::mint = asset_mint.key(),
        token::authority = vault,
        token::token_program = asset_token_program,
        constraint = vault.vault_token_account == vault_token_account.key(),
    )]
    pub vault_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        token::mint = asset_mint.key(),
    )]
    pub recipient_token_account: InterfaceAccount<'info, TokenAccount>,

    pub asset_token_program: Interface<'info, TokenInterface>,
}

impl<'info> WithdrawAssets<'info> {
    pub fn transfer_assets_to_authority(&mut self, amount: u64) -> Result<()> {
        let seeds: &[&[&[u8]]] = &[&[
            VAULT_CONFIG_SEED,
            self.vault.share_mint.as_ref(),
            &[self.vault.bump],
        ]];

        let cpi_accounts = TransferChecked {
            from: self.vault_token_account.to_account_info(),
            mint: self.asset_mint.to_account_info(),
            to: self.recipient_token_account.to_account_info(),
            authority: self.vault.to_account_info(),
        };

        let cpi_ctx =
            CpiContext::new_with_signer(self.asset_token_program.key(), cpi_accounts, seeds);

        token_interface::transfer_checked(cpi_ctx, amount, self.asset_mint.decimals)
    }
}

pub fn handler(ctx: Context<WithdrawAssets>, amount: u64) -> Result<()> {
    ctx.accounts.vault.assert_unpaused_and_initialized()?;
    ctx.accounts.transfer_assets_to_authority(amount)?;
    Ok(())
}
