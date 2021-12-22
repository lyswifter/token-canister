use std::borrow::Cow;
use std::convert::TryFrom;
use std::convert::TryInto;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use std::collections::VecDeque;

use serde::{
    Deserialize, Serialize,
};

use ic_types::CanisterId;
use on_wire::{FromWire, IntoWire};
use candid::CandidType;
use ic_crypto_sha::Sha256;
use dfn_protobuf::ProtoBuf;

use crate::HashOf;
use crate::TimeStamp;
use crate::types::{Memo, Operation, Transaction};
use crate::print;

/// Position of a block in the chain. The first block has position 0.
pub type BlockHeight = u64;

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
    pub fn hash(&self) -> HashOf<Self> {
        let mut state = Sha256::new();
        state.write(&self.0);
        HashOf::new(state.finish())
    }

    pub fn decode(&self) -> Result<Block, String> {
        let bytes = self.0.to_vec();
        Ok(ProtoBuf::from_bytes(bytes)?.get())
    }

    pub fn size_bytes(&self) -> usize {
        self.0.len()
    }
}

/// A transaction with the metadata the canister generated attached to it
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub parent_hash: Option<HashOf<EncodedBlock>>,
    pub transaction: Transaction,
    /// Nanoseconds since the Unix epoch.
    pub timestamp: TimeStamp,
}

impl Block {
    pub fn new(
        parent_hash: Option<HashOf<EncodedBlock>>,
        operation: Operation,
        memo: Memo,
        created_at_time: TimeStamp, // transaction timestamp
        timestamp: TimeStamp,       // block timestamp
    ) -> Result<Self, String> {
        let transaction = Transaction {
            operation,
            memo,
            created_at_time,
        };
        Ok(Self::new_from_transaction(
            parent_hash,
            transaction,
            timestamp,
        ))
    }

    pub fn new_from_transaction(
        parent_hash: Option<HashOf<EncodedBlock>>,
        transaction: Transaction,
        timestamp: TimeStamp,
    ) -> Self {
        Self {
            parent_hash,
            transaction,
            timestamp,
        }
    }

    pub fn encode(self) -> Result<EncodedBlock, String> {
        let slice = ProtoBuf::new(self).into_bytes()?.into_boxed_slice();
        Ok(EncodedBlock(slice))
    }

    pub fn parent_hash(&self) -> Option<HashOf<EncodedBlock>> {
        self.parent_hash
    }

    pub fn transaction(&self) -> Cow<Transaction> {
        Cow::Borrowed(&self.transaction)
    }

    pub fn timestamp(&self) -> TimeStamp {
        self.timestamp
    }
}

/// Stores a chain of transactions with their metadata
#[derive(Serialize, Deserialize, Debug)]
pub struct Blockchain {
    pub blocks: Vec<EncodedBlock>,
    pub last_hash: Option<HashOf<EncodedBlock>>,

    /// The timestamp of the most recent block. Must be monotonically
    /// non-decreasing.
    pub last_timestamp: TimeStamp,
    // pub archive: Arc<RwLock<Option<Archive>>>,

    /// How many blocks have been sent to the archive
    pub num_archived_blocks: u64,
}

impl Default for Blockchain {
    fn default() -> Self {
        Self {
            blocks: vec![],
            last_hash: None,
            last_timestamp: SystemTime::UNIX_EPOCH.into(),
            // archive: Arc::new(RwLock::new(None)),
            num_archived_blocks: 0,
        }
    }
}

impl Blockchain {
    pub fn add_block(&mut self, block: Block) -> Result<BlockHeight, String> {
        let raw_block = block.clone().encode()?;
        self.add_block_with_encoded(block, raw_block)
    }

    pub fn add_block_with_encoded(
        &mut self,
        block: Block,
        encoded_block: EncodedBlock,
    ) -> Result<BlockHeight, String> {
        if block.parent_hash != self.last_hash {
            return Err("Cannot apply block because its parent hash doesn't match.".to_string());
        }
        if block.timestamp < self.last_timestamp {
            return Err(
                "Cannot apply block because its timestamp is older than the previous tip."
                    .to_owned(),
            );
        }
        self.last_hash = Some(encoded_block.hash());
        self.last_timestamp = block.timestamp;
        self.blocks.push(encoded_block);
        Ok(self.chain_length().checked_sub(1).unwrap())
    }

    pub fn get(&self, height: BlockHeight) -> Option<&EncodedBlock> {
        if height < self.num_archived_blocks() {
            None
        } else {
            self.blocks
                .get(usize::try_from(height - self.num_archived_blocks()).unwrap())
        }
    }

    pub fn last(&self) -> Option<&EncodedBlock> {
        self.blocks.last()
    }

    pub fn num_archived_blocks(&self) -> u64 {
        self.num_archived_blocks
    }

    pub fn num_unarchived_blocks(&self) -> u64 {
        self.blocks.len().try_into().unwrap()
    }

    pub fn chain_length(&self) -> BlockHeight {
        self.num_archived_blocks() + self.num_unarchived_blocks() as BlockHeight
    }

    pub fn remove_archived_blocks(&mut self, len: usize) {
        // redundant since split_off would panic, but here we can give a more
        // descriptive message
        if len > self.blocks.len() {
            panic!(
                "Asked to remove more blocks than present. Present: {}, to remove: {}",
                self.blocks.len(),
                len
            );
        }
        self.blocks = self.blocks.split_off(len);
        self.num_archived_blocks += len as u64;
    }

    pub fn get_blocks_for_archiving(
        &self,
        trigger_threshold: usize,
        num_blocks_to_archive: usize,
    ) -> VecDeque<EncodedBlock> {
        // Upon reaching the `trigger_threshold` we will archive
        // `num_blocks_to_archive`. For example, when set to (2000, 1000)
        // archiving will trigger when there are 2000 blocks in the ledger and
        // the 1000 oldest bocks will be archived, leaving the remaining 1000
        // blocks in place.
        let num_blocks_before = self.num_unarchived_blocks() as usize;

        if num_blocks_before < trigger_threshold {
            return VecDeque::new();
        }

        let blocks_to_archive: VecDeque<EncodedBlock> =
            VecDeque::from(self.blocks[0..num_blocks_to_archive.min(num_blocks_before)].to_vec());

        print(format!(
            "get_blocks_for_archiving(): trigger_threshold: {}, num_blocks: {}, blocks before archiving: {}, blocks to archive: {}",
            trigger_threshold,
            num_blocks_to_archive,
            num_blocks_before,
            blocks_to_archive.len(),
        ));

        blocks_to_archive
    }
}

/// Argument returned by the tip_of_chain endpoint
pub struct TipOfChainRes {
    pub certification: Option<Vec<u8>>,
    pub tip_index: BlockHeight,
}

pub struct GetBlocksArgs {
    pub start: BlockHeight,
    pub length: usize,
}

impl GetBlocksArgs {
    pub fn new(start: BlockHeight, length: usize) -> Self {
        GetBlocksArgs { start, length }
    }
}

pub struct GetBlocksRes(pub Result<Vec<EncodedBlock>, String>);

pub struct IterBlocksArgs {
    pub start: usize,
    pub length: usize,
}

impl IterBlocksArgs {
    pub fn new(start: usize, length: usize) -> Self {
        IterBlocksArgs { start, length }
    }
}

pub struct IterBlocksRes(pub Vec<EncodedBlock>);

// These is going away soon
pub struct BlockArg(pub BlockHeight);
pub struct BlockRes(pub Option<Result<EncodedBlock, CanisterId>>);

// A helper function for ledger/get_blocks and archive_node/get_blocks endpoints
pub fn get_blocks(
    blocks: &[EncodedBlock],
    range_from_offset: BlockHeight,
    range_from: BlockHeight,
    length: usize,
) -> GetBlocksRes {
    // Inclusive end of the range of *requested* blocks
    let requested_range_to = range_from as usize + length - 1;
    // Inclusive end of the range of *available* blocks
    let range_to = range_from_offset as usize + blocks.len() - 1;
    // Example: If the Node stores 10 blocks beginning at BlockHeight 100, i.e.
    // [100 .. 109] then requesting blocks at BlockHeight < 100 or BlockHeight
    // > 109 is an error
    if range_from < range_from_offset || requested_range_to > range_to {
        return GetBlocksRes(Err(format!("Requested blocks outside the range stored in the archive node. Requested [{} .. {}]. Available [{} .. {}].",
            range_from, requested_range_to, range_from_offset, range_to)));
    }
    // Example: If the node stores blocks [100 .. 109] then BLOCK_HEIGHT_OFFSET
    // is 100 and the Block with BlockHeight 100 is at index 0
    let offset = (range_from - range_from_offset) as usize;
    GetBlocksRes(Ok(blocks[offset..offset + length].to_vec()))
}

// A helper function for ledger/iter_blocks and archive_node/iter_blocks
// endpoints
pub fn iter_blocks(blocks: &[EncodedBlock], offset: usize, length: usize) -> IterBlocksRes {
    let start = std::cmp::min(offset, blocks.len());
    let end = std::cmp::min(start + length, blocks.len());
    let blocks = blocks[start..end].to_vec();
    IterBlocksRes(blocks)
}