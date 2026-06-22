use anchor_lang::prelude::*;
use anchor_spl::token_interface::Mint;

use crate::{
    error::AsyncVaultError,
    state::{Vault, VAULT_CONFIG_SEED},
};

#[derive(AnchorDeserialize, AnchorSerialize)]
pub struct UpdateVaultArgs {
    pub paused: Option<bool>,
    pub fee_recipient: Option<Pubkey>,
}

#[derive(Accounts)]
pub struct UpdateVault<'info> {
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

pub fn handler(ctx: Context<UpdateVault>, args: UpdateVaultArgs) -> Result<()> {
    let vault = &mut ctx.accounts.vault;

    if let Some(paused) = args.paused {
        vault.paused = paused;
    }

    if let Some(fee_recipient) = args.fee_recipient {
        vault.fee_recipient = fee_recipient;
    }

    Ok(())
}
