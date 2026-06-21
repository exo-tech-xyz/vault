mod generated;

pub mod extensions;

#[cfg(feature = "litesvm")]
#[allow(dead_code)]
mod cu_tracker;

pub use generated::{
    accounts::*, errors::*, instructions::*, programs::*, shared, shared::*, types::*,
};

pub use solana_pubkey::Pubkey;

#[cfg(feature = "litesvm")]
pub mod lite {
    use super::*;
    use litesvm;
    use solana_sdk::{signers::Signers, transaction::Transaction};

    pub trait SendTransaction {
        fn send_transaction<T: Signers + ?Sized>(
            self,
            svm: &mut litesvm::LiteSVM,
            payer: &Pubkey,
            signers: &T,
        ) -> litesvm::types::TransactionResult;
    }

    impl SendTransaction for solana_instruction::Instruction {
        fn send_transaction<T: Signers + ?Sized>(
            self,
            svm: &mut litesvm::LiteSVM,
            payer: &Pubkey,
            signers: &T,
        ) -> litesvm::types::TransactionResult {
            let label = instruction_label(&self.program_id, &self.data);
            let tx = Transaction::new_signed_with_payer(
                &[self],
                Some(payer),
                signers,
                svm.latest_blockhash(),
            );
            let result = svm.send_transaction(tx);
            if let (Some(name), Ok(meta)) = (label, result.as_ref()) {
                crate::cu_tracker::record_cu(name, meta.compute_units_consumed);
            }
            result
        }
    }

    /// Maps an instruction's 8-byte Anchor discriminator to a human-readable name
    /// for CU tracking. Returns None for non-vault instructions (e.g. token setup).
    ///
    /// The `DISCRIMINATORS` table is kept in sync with the program IDL by the
    /// `cu_discriminator_map_matches_idl` guard test in the integration-tests crate.
    pub fn instruction_label(program_id: &Pubkey, data: &[u8]) -> Option<&'static str> {
        if *program_id != ASYNC_VAULT_ID || data.len() < 8 {
            return None;
        }
        let disc: [u8; 8] = data[..8].try_into().ok()?;
        const DISCRIMINATORS: &[([u8; 8], &str)] = &[
            (
                ACCEPT_AUTHORITY_INVITATION_DISCRIMINATOR,
                "accept_authority_invitation",
            ),
            (APPROVE_REQUEST_DISCRIMINATOR, "approve_request"),
            (
                CANCEL_QUEUED_DEPOSIT_REQUEST_DISCRIMINATOR,
                "cancel_queued_deposit_request",
            ),
            (
                CANCEL_QUEUED_REDEMPTION_REQUEST_DISCRIMINATOR,
                "cancel_queued_redemption_request",
            ),
            (CANCEL_REQUEST_DISCRIMINATOR, "cancel_request"),
            (CLAIM_DISCRIMINATOR, "claim"),
            (CLOSE_VAULT_DISCRIMINATOR, "close_vault"),
            (
                CREATE_DEPOSIT_REQUEST_DISCRIMINATOR,
                "create_deposit_request",
            ),
            (CREATE_REDEEM_REQUEST_DISCRIMINATOR, "create_redeem_request"),
            (CREATE_VAULT_DISCRIMINATOR, "create_vault"),
            (
                INITIALIZE_DEPOSIT_FEE_DISCRIMINATOR,
                "initialize_deposit_fee",
            ),
            (
                INITIALIZE_MIN_REDEMPTION_DISCRIMINATOR,
                "initialize_min_redemption",
            ),
            (
                INITIALIZE_MIN_SUBSCRIPTION_DISCRIMINATOR,
                "initialize_min_subscription",
            ),
            (
                INITIALIZE_PAUSABLE_REDEMPTIONS_DISCRIMINATOR,
                "initialize_pausable_redemptions",
            ),
            (
                INITIALIZE_PAUSABLE_SUBSCRIPTIONS_DISCRIMINATOR,
                "initialize_pausable_subscriptions",
            ),
            (
                INITIALIZE_REDEMPTION_QUEUE_DISCRIMINATOR,
                "initialize_redemption_queue",
            ),
            (
                INITIALIZE_SUBSCRIPTION_QUEUE_DISCRIMINATOR,
                "initialize_subscription_queue",
            ),
            (INITIALIZE_VAULT_DISCRIMINATOR, "initialize_vault"),
            (
                INITIALIZE_WITHDRAWAL_FEE_DISCRIMINATOR,
                "initialize_withdrawal_fee",
            ),
            (INVITE_NEW_AUTHORITY_DISCRIMINATOR, "invite_new_authority"),
            (REJECT_REQUEST_DISCRIMINATOR, "reject_request"),
            (SET_OPERATOR_DISCRIMINATOR, "set_operator"),
            (SHUTDOWN_VAULT_DISCRIMINATOR, "shutdown_vault"),
            (
                SKIP_CANCELED_QUEUE_REQUEST_DISCRIMINATOR,
                "skip_canceled_queue_request",
            ),
            (UPDATE_DEPOSIT_FEE_DISCRIMINATOR, "update_deposit_fee"),
            (UPDATE_MIN_REDEMPTION_DISCRIMINATOR, "update_min_redemption"),
            (
                UPDATE_MIN_SUBSCRIPTION_DISCRIMINATOR,
                "update_min_subscription",
            ),
            (
                UPDATE_PAUSABLE_REDEMPTIONS_DISCRIMINATOR,
                "update_pausable_redemptions",
            ),
            (
                UPDATE_PAUSABLE_SUBSCRIPTIONS_DISCRIMINATOR,
                "update_pausable_subscriptions",
            ),
            (UPDATE_VAULT_DISCRIMINATOR, "update_vault"),
            (UPDATE_VAULT_NAV_DISCRIMINATOR, "update_vault_nav"),
            (UPDATE_WITHDRAWAL_FEE_DISCRIMINATOR, "update_withdrawal_fee"),
            (WITHDRAW_ASSETS_DISCRIMINATOR, "withdraw_assets"),
        ];
        DISCRIMINATORS
            .iter()
            .find(|(d, _)| *d == disc)
            .map(|(_, n)| *n)
    }
}

#[cfg(feature = "solana-sdk")]
pub mod sdk {
    use super::*;

    pub fn program_id() -> solana_sdk::pubkey::Pubkey {
        solana_sdk::pubkey::Pubkey::new_from_array(ASYNC_VAULT_ID.to_bytes())
    }

    pub trait IntoSdkInstruction {
        fn into_sdk_instruction(self) -> solana_sdk::instruction::Instruction;
    }

    impl IntoSdkInstruction for solana_instruction::Instruction {
        fn into_sdk_instruction(self) -> solana_sdk::instruction::Instruction {
            solana_sdk::instruction::Instruction {
                program_id: solana_sdk::pubkey::Pubkey::new_from_array(self.program_id.to_bytes()),
                accounts: self
                    .accounts
                    .into_iter()
                    .map(|meta| solana_sdk::instruction::AccountMeta {
                        pubkey: solana_sdk::pubkey::Pubkey::new_from_array(meta.pubkey.to_bytes()),
                        is_signer: meta.is_signer,
                        is_writable: meta.is_writable,
                    })
                    .collect(),
                data: self.data,
            }
        }
    }
}
