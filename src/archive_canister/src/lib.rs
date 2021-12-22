pub mod archive;
pub mod spawn;

use serde::{
    de::{Deserializer, MapAccess, Visitor},
    ser::SerializeMap,
    Deserialize, Serialize, Serializer,
};
use candid::CandidType;

#[derive(
    Serialize, Deserialize, CandidType, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(transparent)]
pub struct EncodedBlock(pub Box<[u8]>);

impl From<Box<[u8]>> for EncodedBlock {
    fn from(bytes: Box<[u8]>) -> Self {
        Self(bytes)
    }
}

impl EncodedBlock {
    // pub fn hash(&self) -> HashOf<Self> {
    //     let mut state = Sha256::new();
    //     state.write(&self.0);
    //     HashOf::new(state.finish())
    // }

    // pub fn decode(&self) -> Result<Block, String> {
    //     let bytes = self.0.to_vec();
    //     Ok(ProtoBuf::from_bytes(bytes)?.get())
    // }

    pub fn size_bytes(&self) -> usize {
        self.0.len()
    }
}