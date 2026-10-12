use aurora_refiner_types::bus_message::bus_serde::{DecodeError, FromCbor};

#[derive(serde::Deserialize)]
pub struct NEARBlock {
    pub block: BlockView,
}

#[derive(serde::Deserialize)]
pub struct BlockView {
    pub header: BlockHeaderView,
}

#[derive(serde::Deserialize)]
pub struct BlockHeaderView {
    pub epoch_id: near_kit::CryptoHash,
}

impl FromCbor for NEARBlock {
    fn from_cbor(value: serde_cbor::Value) -> Result<Self, DecodeError> {
        let json_comp = Vec::<u8>::from_cbor(value)?;
        let json_bytes = aurora_refiner_types::bus_message::compression::decompress(&json_comp)?;
        let result = serde_json::from_slice(&json_bytes)?;

        Ok(result)
    }
}
