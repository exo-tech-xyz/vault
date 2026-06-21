use anchor_lang::prelude::*;

#[error_code]
pub enum AsyncVaultError {
    #[msg("Initial price cannot be zero")]
    InvalidInitialPrice,
    #[msg("Unauthorized signer")]
    UnauthorizedSigner,
    #[msg("Vault is not initialized")]
    UninitializedVault,
    #[msg("Vault is paused")]
    PausedVault,
    #[msg("Vault is already initialized")]
    VaultAlreadyInitialized,
    #[msg("Extension is already initialized")]
    ExtensionAlreadyInitialized,
    #[msg("Extension is not initialized")]
    UninitializedExtension,
    #[msg("Invalid extension data")]
    InvalidExtensionData,
    #[msg("Fee basis points exceed maximum")]
    FeeBpsExceeded,
    #[msg("Arithmetic error")]
    ArithmeticError,
    #[msg("Mints should be different.")]
    MintsShouldBeDifferent,
    #[msg("Share mint supply should be zero.")]
    ShareMintSupplyShouldBeZero,
    #[msg("No pending authority invitation")]
    NoPendingAuthority,
    #[msg("Pending Vault is not valid.")]
    InvalidPendingVault,
    #[msg("Pending Shares Vault is not valid.")]
    InvalidPendingSharesVault,
    #[msg("Fee recipient account must be provided as a remaining account when fee > 0.")]
    MissingFeeRecipient,
    #[msg("Fee recipient account is invalid.")]
    InvalidFeeRecipient,
    #[msg("Request current state is not valid.")]
    RequestInvalidState,
    #[msg("Request is not pending.")]
    RequestIsNotPending,
    #[msg("Invalid request type for this instruction.")]
    InvalidRequestType,
    #[msg("A required optional account was not provided.")]
    MissingRequiredAccount,
    #[msg("Asset mint has invalid extensions.")]
    InvalidAssetMintExtensions,
    #[msg("Asset mint is not valid.")]
    InvalidAssetMint,
    #[msg("Share mint is not valid.")]
    InvalidShareMint,
    #[msg("Request address is not valid.")]
    InvalidRequest,
    #[msg("Request is not in a Pending state.")]
    RequestNotPending,
    #[msg("Request is not in a Claimable state.")]
    RequestNotClaimable,
    #[msg("Subscriptions are paused")]
    SubscriptionsPaused,
    #[msg("Redemptions are paused")]
    RedemptionsPaused,
    #[msg("Deposit request is not next in the subscription queue")]
    SubscriptionQueueOutOfOrder,
    #[msg("Request is not in a Canceled state.")]
    RequestIsNotCanceled,
    #[msg("Queued deposit requests must be canceled via cancel_queued_deposit_request.")]
    MustUseCancelQueuedDepositRequest,
    #[msg("Redeem request is not next in the redemption queue")]
    RedemptionQueueOutOfOrder,
    #[msg("Queued redeem requests must be canceled via cancel_queued_redemption_request.")]
    MustUseCancelQueuedRedemptionRequest,
    #[msg("Deposit amount is below the minimum subscription threshold")]
    SubscriptionAmountBelowMinimum,
    #[msg("Redemption amount is below the minimum redemption threshold")]
    RedemptionAmountBelowMinimum,
    #[msg("Nav is not set.")]
    NavIsNotSet,
    #[msg("Redeem shares amount too small.")]
    InsufficientRedeemAmount,
    #[msg("Approval does not match the live request instance.")]
    ApprovalRequestMismatch,
    #[msg("Deposit amount too small.")]
    InsufficientDepositAmount,
    #[msg("Share mint has invalid extensions.")]
    InvalidShareMintExtensions,
    #[msg("Vault is shutting down.")]
    VaultIsClosing,
    #[msg("Vault shutdown has already started.")]
    VaultAlreadyClosing,
    #[msg("Vault shutdown must start before final closure.")]
    VaultMustBeShutdownBeforeClosing,
    #[msg("Vault has pending async requests.")]
    VaultHasPendingAsyncRequests,
    #[msg("Vault has a nonzero total asset balance.")]
    VaultHasOutstandingAssetBalance,
    #[msg("Share mint supply must be zero before closing the vault.")]
    ShareMintSupplyMustBeZeroBeforeClosing,
    #[msg("Subscription queue must be fully processed before closing the vault.")]
    SubscriptionQueueNotDrained,
    #[msg("Redemption queue must be fully processed before closing the vault.")]
    RedemptionQueueNotDrained,
}

impl From<vault_common::VaultMathError> for AsyncVaultError {
    fn from(err: vault_common::VaultMathError) -> Self {
        match err {
            vault_common::VaultMathError::ArithmeticError => AsyncVaultError::ArithmeticError,
            vault_common::VaultMathError::FeeBpsLimitReached => AsyncVaultError::FeeBpsExceeded,
        }
    }
}
