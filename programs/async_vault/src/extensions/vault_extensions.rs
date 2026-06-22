use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    extensions::{
        get_extension_bytes, has_extension, tlv_used_len, update_extension, write_extension,
        TLV_HEADER_SIZE, TLV_START,
    },
    state::Vault,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum ExtensionType {
    DepositFee = 1,
    WithdrawalFee = 2,
    PausableSubscriptions = 3,
    PausableRedemptions = 4,
    SubscriptionQueue = 5,
    RedemptionQueue = 6,
    MinSubscription = 7,
    MinRedemption = 8,
}

impl ExtensionType {
    pub fn try_from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::DepositFee),
            2 => Some(Self::WithdrawalFee),
            3 => Some(Self::PausableSubscriptions),
            4 => Some(Self::PausableRedemptions),
            5 => Some(Self::SubscriptionQueue),
            6 => Some(Self::RedemptionQueue),
            7 => Some(Self::MinSubscription),
            8 => Some(Self::MinRedemption),
            _ => None,
        }
    }

    pub const fn data_len(&self) -> usize {
        match self {
            Self::DepositFee | Self::WithdrawalFee => 9,
            Self::PausableSubscriptions | Self::PausableRedemptions => 1,
            Self::SubscriptionQueue | Self::RedemptionQueue => 16,
            Self::MinSubscription | Self::MinRedemption => 8,
        }
    }

    pub const fn tlv_len(&self) -> usize {
        TLV_HEADER_SIZE + self.data_len()
    }
}

/// Implemented by every extension data struct. Associates the struct with its
/// [`ExtensionType`] discriminant and the total TLV byte size it occupies on-chain,
/// enabling the generic init/update/read helpers to operate on any extension uniformly.
pub trait VaultExtension: bytemuck::Pod + bytemuck::Zeroable {
    /// The TLV discriminant that identifies this extension in vault account data.
    const EXTENSION_TYPE: ExtensionType;
    /// Byte count of this extension's data payload.
    const DATA_SIZE: usize = Self::EXTENSION_TYPE.data_len();
    /// Total bytes consumed in the TLV region: `TLV_HEADER_SIZE + DATA_SIZE`.
    const TLV_SIZE: usize = TLV_HEADER_SIZE + Self::DATA_SIZE;
}

/// Shared accounts for all update-extension instructions.
///
/// Enforces that `authority` matches `vault.authority`. Because update instructions
/// do not resize the account, no payer or system program is required.
#[derive(Accounts)]
pub struct BasicExtensionAccounts<'info> {
    pub authority: Signer<'info>,
    #[account(
        mut,
        constraint = authority.key() == vault.authority @ AsyncVaultError::UnauthorizedSigner,
    )]
    pub vault: Account<'info, Vault>,
}

/// Writes a new TLV extension entry to the vault's extension region.
///
/// Errors if the vault is already initialized or the extension type is already present.
pub fn init_vault_extension<E: VaultExtension>(
    vault_info: &AccountInfo,
    vault: &Vault,
    value: &E,
) -> Result<()> {
    vault.assert_uninitialized()?;
    let mut data = vault_info
        .data
        .try_borrow_mut()
        .map_err(|_| ProgramError::AccountBorrowFailed)?;
    let tlv_data = &mut data[TLV_START..];
    require!(
        !has_extension(tlv_data, E::EXTENSION_TYPE),
        AsyncVaultError::ExtensionAlreadyInitialized
    );
    let write_offset = tlv_used_len(tlv_data);
    write_extension(
        tlv_data,
        write_offset,
        E::EXTENSION_TYPE,
        bytemuck::bytes_of(value),
    )
}

/// Overwrites an existing TLV extension entry in the vault's extension region.
///
/// Errors if the extension type is not present.
pub fn update_vault_extension<E: VaultExtension>(
    vault_info: &AccountInfo,
    value: &E,
) -> Result<()> {
    let mut data = vault_info
        .data
        .try_borrow_mut()
        .map_err(|_| ProgramError::AccountBorrowFailed)?;
    let tlv_data = &mut data[TLV_START..];
    update_extension(tlv_data, E::EXTENSION_TYPE, bytemuck::bytes_of(value))
}

/// Reads an extension from raw vault account data.
///
/// Returns `Ok(None)` if the account has no TLV region or the extension type is absent.
pub fn read_vault_extension<E: VaultExtension>(account_data: &[u8]) -> Result<Option<E>> {
    if account_data.len() <= TLV_START {
        return Ok(None);
    }
    let tlv_data = &account_data[TLV_START..];
    get_extension_bytes(tlv_data, E::EXTENSION_TYPE)
        .map(|bytes| {
            bytemuck::try_pod_read_unaligned::<E>(bytes)
                .map_err(|_| crate::error::AsyncVaultError::InvalidExtensionData.into())
        })
        .transpose()
}
