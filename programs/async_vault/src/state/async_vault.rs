use anchor_lang::prelude::*;

use crate::error::AsyncVaultError;

#[account]
#[derive(InitSpace)]
pub struct Vault {
    pub asset_mint: Pubkey,
    /// share mint address
    pub share_mint: Pubkey,
    /// token account holding confirmed vault assets
    pub vault_token_account: Pubkey,
    /// authority that can sign permissioned instructions
    pub authority: Pubkey,
    /// pubkey that is required to own the TokenAccount fees are sent to
    pub fee_recipient: Pubkey,
    /// paused
    pub paused: bool,
    /// irreversible wind-down state; blocks new subscriptions while allowing exits
    pub closing: bool,
    /// once a vault is initialized, no extensions can be added
    pub initialized: bool,
    /// token account holding assets from deposits awaiting share issuance
    pub pending_vault: Pubkey,
    /// net asset value (assets per share), default 0 until first NAV update
    pub nav: u128,
    /// nav version, incremented on each NAV update
    pub nav_version: u64,
    /// count of pending async deposit/withdrawal requests
    pub pending_async_requests: u16,
    /// virtual vault asset balance, accounts for tokens that may
    /// have been withdrawn by the vault authority
    pub total_asset_balance: u64,
    pub reserve_bump: u8,
    pub pending_vault_bump: u8,
    pub bump: u8,
    // Used for updating the vault authority (New Authority)
    pub pending_authority: Option<Pubkey>,
}

impl Vault {
    pub fn assert_unpaused_and_initialized(&self) -> Result<()> {
        require!(self.initialized, AsyncVaultError::UninitializedVault);
        require!(!self.paused, AsyncVaultError::PausedVault);
        Ok(())
    }

    pub fn assert_active(&self) -> Result<()> {
        self.assert_unpaused_and_initialized()?;
        require!(!self.closing, AsyncVaultError::VaultIsClosing);
        Ok(())
    }

    pub fn assert_not_closing(&self) -> Result<()> {
        require!(!self.closing, AsyncVaultError::VaultIsClosing);
        Ok(())
    }

    pub fn assert_uninitialized(&self) -> Result<()> {
        require!(!self.initialized, AsyncVaultError::VaultAlreadyInitialized);
        Ok(())
    }
}
