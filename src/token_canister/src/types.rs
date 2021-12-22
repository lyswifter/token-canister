

use crate::account_identifier::{ AccountIdentifier };
use crate::ic_token::TOKENs;
use crate::TimeStamp;
use crate::HashOf;
// use ic_types::CanisterId;

use candid::CandidType;
use ic_crypto_sha::Sha256;

use serde::{
    de::{Deserializer, MapAccess, Visitor},
    ser::SerializeMap,
    Deserialize, Serialize, Serializer,
};


/// An operation with the metadata the client generated attached to it
#[derive(
    Serialize, Deserialize, CandidType, Clone, Hash, Debug, PartialEq, Eq, PartialOrd, Ord,
)]
pub struct Transaction {
    pub operation: Operation,
    pub memo: Memo,

    /// The time this transaction was created.
    pub created_at_time: TimeStamp,
}

impl Transaction {
    pub fn new(
        from: AccountIdentifier,
        to: AccountIdentifier,
        amount: TOKENs,
        fee: TOKENs,
        memo: Memo,
        created_at_time: TimeStamp,
    ) -> Self {
        let operation = Operation::Transfer {
            from,
            to,
            amount,
            fee,
        };
        Transaction {
            operation,
            memo,
            created_at_time,
        }
    }

    pub fn hash(&self) -> HashOf<Self> {
        let mut state = Sha256::new();
        state.write(&serde_cbor::ser::to_vec_packed(&self).unwrap());
        HashOf::new(state.finish())
    }
}

/// An operation which modifies account balances
#[derive(
    Serialize, Deserialize, CandidType, Clone, Hash, Debug, PartialEq, Eq, PartialOrd, Ord,
)]
pub enum Operation {
    Burn {
        from: AccountIdentifier,
        amount: TOKENs,
    },
    Mint {
        to: AccountIdentifier,
        amount: TOKENs,
    },
    Transfer {
        from: AccountIdentifier,
        to: AccountIdentifier,
        amount: TOKENs,
        fee: TOKENs,
    },
}

#[derive(
    Serialize, Deserialize, CandidType, Clone, Copy, Hash, Debug, PartialEq, Eq, PartialOrd, Ord,
)]
pub struct Memo(pub u64);

impl Default for Memo {
    fn default() -> Memo {
        Memo(0)
    }
}

// #[derive(Serialize, Deserialize, CandidType, Clone, Debug, PartialEq, Eq)]
// pub struct ArchiveOptions {
//     /// The number of blocks which, when exceeded, will trigger an archiving
//     /// operation
//     pub trigger_threshold: usize,
//     /// The number of blocks to archive when trigger threshold is exceeded
//     pub num_blocks_to_archive: usize,
//     pub node_max_memory_size_bytes: Option<usize>,
//     pub max_message_size_bytes: Option<usize>,
//     pub controller_id: CanisterId,
// }

// #[derive(Serialize, Deserialize, Debug)]
// pub struct Archive {
//     // List of Archive Nodes
//     nodes: Vec<CanisterId>,

//     controller_id: CanisterId,

//     // BlockHeights of Blocks stored in each archive node.

//     // We need this because Blocks are stored in encoded format as
//     // EncodedBlocks, and different EncodedBlocks may have different lengths.
//     // Moreover, archive node capacity is specified in bytes instead of a fixed
//     // number of Blocks. Thus, it is not possible to statically compute how
//     // many EncodedBlocks will fit into an archive node -- the actual number
//     // will vary slightly.

//     // To facilitate lookup by index we will keep track of the number of Blocks
//     // stored in each archive. We store an inclusive range [from, to]. Thus,
//     // the range [0..9] means we store 10 blocks with indices from 0 to 9
//     nodes_block_ranges: Vec<(u64, u64)>,

//     // Maximum amount of data that can be stored in an Archive Node canister
//     node_max_memory_size_bytes: usize,

//     // Maximum inter-canister message size in bytes
//     max_message_size_bytes: usize,

//     /// How many blocks have been sent to the archive
//     num_archived_blocks: u64,

//     /// The number of blocks which, when exceeded, will trigger an archiving
//     /// operation
//     pub trigger_threshold: usize,
//     /// The number of blocks to archive when trigger threshold is exceeded
//     pub num_blocks_to_archive: usize,
// }

// /// This error type should only be returned in the case where an await has been
// /// passed but we do not think that the archive canister has received the blocks
// pub struct FailedToArchiveBlocks(pub String);