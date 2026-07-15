use anchor_lang::{prelude::*, solana_program::entrypoint::ProgramResult};
use anchor_spl::token_2022::spl_token_2022::{
    self,
    extension::{BaseStateWithExtensions, StateWithExtensions},
};

use crate::error::AsyncVaultError;

/// Validates the extensions on the asset mint to ensure compatibility with the
/// Async Vault program. This is checked during Vault Creation and Deposit/DepositRequest.
/// - TransferFeeConfig can be enabled, but must be 0. Many stablecoins have
/// TransferFeeConfig enabled with a 0 fee, so this is important to support.
pub fn validate_asset_mint_extensions_from_acct_info(mint_acct: &AccountInfo) -> Result<()> {
    if mint_acct.owner != &spl_token_2022::ID {
        return Ok(());
    }
    let mint_data = mint_acct.try_borrow_data()?;
    let mint = StateWithExtensions::<spl_token_2022::state::Mint>::unpack(&mint_data)?;

    // Validate: Mint has 0 transfer fees
    if let Ok(transfer_fee_config) =
        mint.get_extension::<spl_token_2022::extension::transfer_fee::TransferFeeConfig>()
    {
        let clock = Clock::get()?;
        let transfer_fee_bps = u16::from_le_bytes(
            transfer_fee_config
                .get_epoch_fee(clock.epoch)
                .transfer_fee_basis_points
                .0,
        );
        if transfer_fee_bps != 0 {
            return Err(AsyncVaultError::InvalidAssetMintExtensions.into());
        }
    }

    Ok(())
}

/// Validates the share mint's extensions against the vault's public mint/burn share lifecycle.
/// Rejects extensions that would make the share lifecycle or terminal vault closure unsafe.
pub fn validate_share_mint_extensions_from_acct_info(mint_acct: &AccountInfo) -> Result<()> {
    if mint_acct.owner != &spl_token_2022::ID {
        return Ok(());
    }
    let mint_data = mint_acct.try_borrow_data()?;
    let mint = StateWithExtensions::<spl_token_2022::state::Mint>::unpack(&mint_data)?;

    if mint
        .get_extension::<spl_token_2022::extension::confidential_mint_burn::ConfidentialMintBurn>()
        .is_ok()
    {
        return Err(AsyncVaultError::InvalidShareMintExtensions.into());
    }

    if mint
        .get_extension::<spl_token_2022::extension::mint_close_authority::MintCloseAuthority>()
        .is_ok()
    {
        // A different close authority could delete the zero-supply share mint before
        // close_vault finishes, leaving the vault cleanup accounts stranded.
        return Err(AsyncVaultError::InvalidShareMintExtensions.into());
    }

    Ok(())
}

/// Read the `owner` Pubkey of a TokenAccount without deserializing the whole account
/// and validate against an expected owner.
pub fn validate_token_account_owner(info: &AccountInfo, expected_owner: &Pubkey) -> ProgramResult {
    let data = info.try_borrow_data()?;
    if data.len() < 64 {
        return Err(ProgramError::InvalidAccountData);
    }
    let owner = Pubkey::new_from_array(
        data[32..64]
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?,
    );
    if owner.ne(&expected_owner) {
        return Err(ProgramError::InvalidAccountOwner);
    }
    Ok(())
}

/// Converts an asset amount into shares using the supplied NAV.
/// Rounding: floored
/// `shares = net_amount * 10^decimals / nav`
pub fn calculate_shares(nav: u128, decimals: u8, net_amount: u64) -> Result<u64> {
    let precision = 10u128
        .checked_pow(decimals as u32)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    let shares = u128::from(net_amount)
        .checked_mul(precision)
        .ok_or(AsyncVaultError::ArithmeticError)?
        .checked_div(nav)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    Ok(u64::try_from(shares).map_err(|_| AsyncVaultError::ArithmeticError)?)
}

/// Converts a share amount into assets using the supplied NAV.
/// Rounding: floored
/// `assets = share_amount * nav / 10^decimals`
pub fn calculate_assets(nav: u128, decimals: u8, share_amount: u64) -> Result<u64> {
    let precision = 10u128
        .checked_pow(decimals as u32)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    let assets = u128::from(share_amount)
        .checked_mul(nav)
        .ok_or(AsyncVaultError::ArithmeticError)?
        .checked_div(precision)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    if assets.eq(&0u128) {
        return Err(AsyncVaultError::ArithmeticError.into());
    }
    Ok(u64::try_from(assets).map_err(|_| AsyncVaultError::ArithmeticError)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test_case(1_000_000u128, 6u8, 1_000_000u64 => 1_000_000u64; "one_to_one")]
    #[test_case(2_000_000u128, 6u8, 2_000_000u64 => 1_000_000u64; "nav_above_one")]
    #[test_case(3_000_000u128, 6u8, 1_000_000u64 => 333_333u64; "fractional_truncates")]
    #[test_case(1_000_000u128, 6u8, 0u64 => 0u64; "zero_amount")]
    #[test_case(100_000_000u128, 8u8, 100_000_000u64 => 100_000_000u64; "different_decimals")]
    #[test_case(1_000_000u128, 6u8, u64::MAX => u64::MAX; "large_amount_no_overflow")]
    fn calculate_shares_success(nav: u128, decimals: u8, amount: u64) -> u64 {
        calculate_shares(nav, decimals, amount).unwrap()
    }

    #[test]
    fn calculate_shares_zero_nav_errors() {
        assert!(calculate_shares(0, 6, 1_000_000).is_err());
    }

    #[test_case(1_000_000u128, 6u8, 1_000_000u64 => 1_000_000u64; "one_to_one")]
    #[test_case(2_000_000u128, 6u8, 1_000_000u64 => 2_000_000u64; "nav_above_one")]
    #[test_case(3_000_000u128, 6u8, 333_333u64 => 999_999u64; "fractional_truncates")]
    #[test_case(100_000_000u128, 8u8, 100_000_000u64 => 100_000_000u64; "different_decimals")]
    fn calculate_assets_success(nav: u128, decimals: u8, shares: u64) -> u64 {
        calculate_assets(nav, decimals, shares).unwrap()
    }

    #[test]
    fn calculate_assets_zero_nav_errors() {
        assert!(calculate_assets(0, 6, 1_000_000).is_err());
    }

    #[test]
    fn calculate_assets_zero_shares_errors() {
        assert!(calculate_assets(1_000_000, 6, 0).is_err());
    }
}
