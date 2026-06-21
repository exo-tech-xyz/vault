use anchor_lang::prelude::*;
use anchor_spl::token_interface::Mint;

use crate::{
    error::AsyncVaultError,
    extensions::pausable_redemptions::check_redemptions_paused,
    state::{Vault, VAULT_CONFIG_SEED},
};

#[derive(Accounts)]
pub struct ShutdownVault<'info> {
    pub authority: Signer<'info>,

    pub share_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        constraint = authority.key() == vault.authority @ AsyncVaultError::UnauthorizedSigner,
        seeds = [VAULT_CONFIG_SEED, share_mint.key().as_ref()],
        bump = vault.bump,
    )]
    pub vault: Account<'info, Vault>,
}

/// Starts an irreversible wind-down. New subscriptions and reserve withdrawals are blocked,
/// while redemptions and outstanding request settlement remain available until final closure.
pub fn handler(ctx: Context<ShutdownVault>) -> Result<()> {
    let vault = &mut ctx.accounts.vault;
    require!(!vault.paused, AsyncVaultError::PausedVault);
    require!(!vault.closing, AsyncVaultError::VaultAlreadyClosing);
    check_redemptions_paused(&vault.to_account_info())?;

    vault.closing = true;
    Ok(())
}
