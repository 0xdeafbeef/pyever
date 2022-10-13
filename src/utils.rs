use anyhow::Result;
use nekoton as nt;

pub fn extract_public_key(
    account_stuff: &ton_block::AccountStuff,
) -> Result<ed25519_dalek::PublicKey> {
    use nt::core::ton_wallet::{highload_wallet_v2, wallet_v3};

    let state_init = match &account_stuff.storage.state {
        ton_block::AccountState::AccountActive { state_init, .. } => state_init,
        _ => return Err(nt::abi::ExtractionError::AccountIsNotActive.into()),
    };
    let data = match &state_init.data {
        Some(data) => data,
        None => return Err(nt::abi::ExtractionError::AccountDataNotFound.into()),
    };

    if let Some(code) = &state_init.code {
        let code_hash = code.repr_hash();
        if wallet_v3::is_wallet_v3(&code_hash) {
            return wallet_v3::InitData::try_from(data).and_then(|init_data| {
                Ok(ed25519_dalek::PublicKey::from_bytes(
                    init_data.public_key.as_slice(),
                )?)
            });
        } else if highload_wallet_v2::is_highload_wallet_v2(&code_hash) {
            return highload_wallet_v2::InitData::try_from(data).and_then(|init_data| {
                Ok(ed25519_dalek::PublicKey::from_bytes(
                    init_data.public_key.as_slice(),
                )?)
            });
        }
    }

    let data = ton_types::SliceData::from(data)
        .get_next_bytes(32)
        .map_err(|_| nt::abi::ExtractionError::CellUnderflow)?;

    Ok(ed25519_dalek::PublicKey::from_bytes(&data)
        .map_err(|_| nt::abi::ExtractionError::InvalidPublicKey)?)
}
