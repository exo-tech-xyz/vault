pub mod fee;
pub mod min_redemption;
pub mod min_subscription;
pub mod pausable_redemptions;
pub mod pausable_subscriptions;
pub mod redemption_queue;
pub mod subscription_queue;

pub const VAULT_TLV_START: usize = 272;
/// Byte offset where request extension TLV data begins (8-byte discriminator + 171 fixed fields).
pub const REQUEST_TLV_START: usize = 179;

const TLV_HEADER_SIZE: usize = 4;

#[derive(Clone, Copy)]
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

#[derive(Clone, Copy)]
#[repr(u16)]
pub enum RequestExtensionType {
    SubscriptionQueueRequest = 1,
    RedemptionQueueRequest = 2,
}

pub fn get_extension_bytes(tlv_data: &[u8], ext_type: ExtensionType) -> Option<&[u8]> {
    get_tlv_bytes(tlv_data, ext_type as u16)
}

/// Returns the TLV value bytes for the given type from a request account's extension region.
pub fn get_request_extension_bytes(
    request_data: &[u8],
    ext_type: RequestExtensionType,
) -> Option<&[u8]> {
    if request_data.len() <= REQUEST_TLV_START {
        return None;
    }
    get_tlv_bytes(&request_data[REQUEST_TLV_START..], ext_type as u16)
}

fn get_tlv_bytes(tlv_data: &[u8], ext_type: u16) -> Option<&[u8]> {
    let mut offset = 0;
    while offset + TLV_HEADER_SIZE <= tlv_data.len() {
        let entry_type = u16::from_le_bytes([tlv_data[offset], tlv_data[offset + 1]]);
        let entry_len = u16::from_le_bytes([tlv_data[offset + 2], tlv_data[offset + 3]]) as usize;
        let value_end = offset + TLV_HEADER_SIZE + entry_len;
        if value_end > tlv_data.len() {
            return None;
        }
        if entry_type == ext_type {
            return Some(&tlv_data[offset + TLV_HEADER_SIZE..value_end]);
        }
        offset = value_end;
    }
    None
}
