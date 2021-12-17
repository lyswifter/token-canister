use candid::CandidType;
use dfn_protobuf::ProtoBuf;
use ic_crypto_sha::Sha256;
use ic_types::{CanisterId, PrincipalId};
use intmap::IntMap;
use lazy_static::lazy_static;
use on_wire::{FromWire, IntoWire};
use phantom_newtype::Id;
use serde::{
    de::{Deserializer, MapAccess, Visitor},
    ser::SerializeMap,
    Deserialize, Serialize, Serializer,
};
use std::borrow::Cow;
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::convert::TryFrom;
use std::convert::TryInto;
use std::fmt;
use std::hash::Hash;
use std::marker::PhantomData;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

pub mod account_identifier;
pub mod http_request;
pub mod ic_token;
pub mod ic_block;
pub mod metrics_encoder;
#[path = "../gen/ic_ledger.pb.v1.rs"]
#[rustfmt::skip]
pub mod protobuf;
pub mod timestamp;
pub mod validate_endpoints;

pub mod archive;

use archive::Archive;
pub use archive::ArchiveOptions;
use dfn_core::api::now;

pub mod spawn;
pub use account_identifier::{AccountIdentifier, Subaccount};
pub use ic_token::{TOKENs, DECIMAL_PLACES, TOKEN_SUBDIVIDABLE_BY, MIN_BURN_AMOUNT, TRANSACTION_FEE};
pub use protobuf::TimeStamp;

use ic_block::{EncodedBlock, Block, Blockchain, EncodedBlock};

// Helper to print messages in magenta
pub fn print<S: std::convert::AsRef<str>>(s: S)
where
    yansi::Paint<S>: std::string::ToString,
{
    dfn_core::api::print(yansi::Paint::magenta(s).to_string());
}

pub const HASH_LENGTH: usize = 32;

#[derive(CandidType, Clone, Hash, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct HashOf<T> {
    inner: Id<T, [u8; HASH_LENGTH]>,
}

impl<T: std::clone::Clone> Copy for HashOf<T> {}

impl<T> HashOf<T> {
    pub fn into_bytes(self) -> [u8; HASH_LENGTH] {
        self.inner.get()
    }

    pub fn new(bs: [u8; HASH_LENGTH]) -> Self {
        HashOf { inner: Id::new(bs) }
    }
}

impl<T> fmt::Display for HashOf<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let res = hex::encode(self.inner.get());
        write!(f, "{}", res)
    }
}

impl<T> FromStr for HashOf<T> {
    type Err = String;
    fn from_str(s: &str) -> Result<HashOf<T>, String> {
        let v = hex::decode(s).map_err(|e| e.to_string())?;
        let slice = v.as_slice();
        match slice.try_into() {
            Ok(ba) => Ok(HashOf::new(ba)),
            Err(_) => Err(format!(
                "Expected a Vec of length {} but it was {}",
                HASH_LENGTH,
                v.len(),
            )),
        }
    }
}

impl<T> Serialize for HashOf<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.to_string())
        } else {
            serializer.serialize_bytes(self.inner.get_ref())
        }
    }
}

impl<'de, T> Deserialize<'de> for HashOf<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct HashOfVisitor<T> {
            phantom: PhantomData<T>,
        }

        impl<'de, T> Visitor<'de> for HashOfVisitor<T> {
            type Value = HashOf<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    formatter,
                    "a hash of type {}: a blob with at most {} bytes",
                    std::any::type_name::<T>(),
                    HASH_LENGTH
                )
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(HashOf::new(
                    v.try_into().expect("hash does not have correct length"),
                ))
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                HashOf::from_str(s).map_err(E::custom)
            }
        }

        if deserializer.is_human_readable() {
            deserializer.deserialize_str(HashOfVisitor {
                phantom: PhantomData,
            })
        } else {
            deserializer.deserialize_bytes(HashOfVisitor {
                phantom: PhantomData,
            })
        }
    }
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

/// Position of a block in the chain. The first block has position 0.
pub type BlockHeight = u64;

pub type Certification = Option<Vec<u8>>;

pub type LedgerBalances = Balances<HashMap<AccountIdentifier, TOKENs>>;

pub trait BalancesStore {
    fn get_balance(&self, k: &AccountIdentifier) -> Option<&TOKENs>;
    // Update balance for an account using function f.
    // Its arg is previous balance or None if not found and
    // return value is the new balance.
    fn update<F>(&mut self, acc: AccountIdentifier, action_on_acc: F)
    where
        F: FnMut(Option<&TOKENs>) -> TOKENs;
}

impl BalancesStore for HashMap<AccountIdentifier, TOKENs> {
    fn get_balance(&self, k: &AccountIdentifier) -> Option<&TOKENs> {
        self.get(k)
    }

    fn update<F>(&mut self, k: AccountIdentifier, mut f: F)
    where
        F: FnMut(Option<&TOKENs>) -> TOKENs,
    {
        match self.entry(k) {
            Occupied(mut entry) => {
                let new_v = f(Some(entry.get()));
                if new_v != TOKENs::ZERO {
                    *entry.get_mut() = new_v;
                } else {
                    entry.remove_entry();
                }
            }
            Vacant(entry) => {
                let new_v = f(None);
                if new_v != TOKENs::ZERO {
                    entry.insert(new_v);
                }
            }
        };
    }
}

/// Describes the state of users accounts at the tip of the chain
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Balances<S: BalancesStore> {
    // This uses a mutable map because we don't want to risk a space leak and we only require the
    // account balances at the tip of the chain
    pub store: S,
    pub icpt_pool: TOKENs,
}

impl<S: Default + BalancesStore> Default for Balances<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Default + BalancesStore> Balances<S> {
    pub fn new() -> Self {
        Self {
            store: S::default(),
            icpt_pool: TOKENs::MAX,
        }
    }

    pub fn add_payment(&mut self, payment: &Operation) {
        match payment {
            Operation::Transfer {
                from,
                to,
                amount,
                fee,
            } => {
                let debit_amount = (*amount + *fee).expect("amount + fee failed");
                self.debit(from, debit_amount);
                self.credit(to, *amount);
                self.icpt_pool += *fee;
            }
            Operation::Burn { from, amount, .. } => {
                self.debit(from, *amount);
                self.icpt_pool += *amount;
            }
            Operation::Mint { to, amount, .. } => {
                self.credit(to, *amount);
                self.icpt_pool -= *amount;
            }
        }
    }

    // Debiting an account will automatically remove it from the `inner`
    // HashMap if the balance reaches zero.
    pub fn debit(&mut self, from: &AccountIdentifier, amount: TOKENs) {
        self.store.update(*from, |prev| {
            let mut balance = match prev {
                Some(x) => *x,
                None => panic!("You tried to withdraw funds from empty account {}", from),
            };
            if balance < amount {
                panic!(
                    "You have tried to spend more than the balance of account {}",
                    from
                );
            }
            balance -= amount;
            balance
        });
    }

    // Crediting an account will automatically add it to the `inner` HashMap if
    // not already present.
    pub fn credit(&mut self, to: &AccountIdentifier, amount: TOKENs) {
        self.store.update(*to, |prev| {
            let mut balance = match prev {
                Some(x) => *x,
                None => TOKENs::ZERO,
            };
            balance += amount;
            balance
        });
    }

    pub fn account_balance(&self, account: &AccountIdentifier) -> TOKENs {
        self.store
            .get_balance(account)
            .cloned()
            .unwrap_or(TOKENs::ZERO)
    }

    /// Returns the total quantity of ICPs that are "in existence" -- that
    /// is, excluding un-minted "potential" ICPs.
    pub fn total_supply(&self) -> TOKENs {
        (TOKENs::MAX - self.icpt_pool).unwrap_or_else(|e| {
            panic!(
                "It is expected that the icpt_pool is always smaller than \
            or equal to ICPTs::MAX, yet subtracting it lead to the following error: {}",
                e
            )
        })
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

impl LedgerBalances {
    // Find the specified number of accounts with lowest balances so that their
    // balances can be reclaimed.
    fn select_accounts_to_trim(&mut self, num_accounts: usize) -> Vec<(TOKENs, AccountIdentifier)> {
        let mut to_trim: std::collections::BinaryHeap<(TOKENs, AccountIdentifier)> =
            std::collections::BinaryHeap::new();

        let mut iter = self.store.iter();

        // Accumulate up to `trim_quantity` accounts
        for (account, balance) in iter.by_ref().take(num_accounts) {
            to_trim.push((*balance, *account));
        }

        for (account, balance) in iter {
            // If any account's balance is lower than the maximum in our set,
            // include that account, and remove the current maximum
            if let Some((greatest_balance, _)) = to_trim.peek() {
                if balance < greatest_balance {
                    to_trim.push((*balance, *account));
                    to_trim.pop();
                }
            }
        }

        to_trim.into_vec()
    }
}

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

#[derive(Serialize, Deserialize, Debug)]
pub struct Ledger {
    pub balances: LedgerBalances,
    pub blockchain: Blockchain,
    // A cap on the maximum number of accounts
    maximum_number_of_accounts: usize,
    // When maximum number of accounts is exceeded, a specified number of
    // accounts with lowest balances are removed
    accounts_overflow_trim_quantity: usize,
    pub minting_account_id: Option<AccountIdentifier>,
    // This is a set of blockheights that have been notified
    #[serde(
        serialize_with = "serialize_int_map",
        deserialize_with = "deserialize_int_map",
        default = "IntMap::new"
    )]
    pub blocks_notified: IntMap<()>,
    /// How long transactions are remembered to detect duplicates.
    pub transaction_window: Duration,
    /// For each transaction, record the block in which the
    /// transaction was created. This only contains transactions from
    /// the last `transaction_window` period.
    transactions_by_hash: BTreeMap<HashOf<Transaction>, BlockHeight>,
    /// The transactions in the transaction window, sorted by block
    /// index / block timestamp. (Block timestamps are monotonically
    /// non-decreasing, so this is the same.)
    transactions_by_height: VecDeque<TransactionInfo>,
    /// Used to prevent non-whitelisted canisters from sending tokens
    send_whitelist: HashSet<CanisterId>,
}

#[derive(Serialize, Deserialize, Debug)]
struct TransactionInfo {
    block_timestamp: TimeStamp,
    transaction_hash: HashOf<Transaction>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            balances: LedgerBalances::default(),
            blockchain: Blockchain::default(),
            maximum_number_of_accounts: 50_000_000,
            accounts_overflow_trim_quantity: 100_000,
            minting_account_id: None,
            blocks_notified: IntMap::new(),
            transaction_window: Duration::from_secs(24 * 60 * 60),
            transactions_by_hash: BTreeMap::new(),
            transactions_by_height: VecDeque::new(),
            send_whitelist: HashSet::new(),
        }
    }
}

impl Ledger {
    /// This creates a block and adds it to the ledger
    pub fn add_payment(
        &mut self,
        memo: Memo,
        payment: Operation,
        created_at_time: Option<TimeStamp>,
    ) -> Result<(BlockHeight, HashOf<EncodedBlock>), String> {
        self.add_payment_with_timestamp(memo, payment, created_at_time, dfn_core::api::now().into())
    }

    /// Internal version of `add_payment` that takes a timestamp, for
    /// testing.
    fn add_payment_with_timestamp(
        &mut self,
        memo: Memo,
        payment: Operation,
        created_at_time: Option<TimeStamp>,
        now: TimeStamp,
    ) -> Result<(BlockHeight, HashOf<EncodedBlock>), String> {
        self.purge_old_transactions(now);

        let created_at_time = created_at_time.unwrap_or(now);

        if created_at_time + self.transaction_window < now {
            return Err("Rejecting expired transaction.".to_owned());
        }

        if created_at_time > now + ic_types::ingress::PERMITTED_DRIFT {
            return Err("Rejecting transaction with timestamp in the future.".to_owned());
        }

        let transaction = Transaction {
            operation: payment.clone(),
            memo,
            created_at_time,
        };

        let transaction_hash = transaction.hash();

        if self.transactions_by_hash.contains_key(&transaction_hash) {
            return Err("Transaction already exists on chain.".to_owned());
        }

        let block = Block::new_from_transaction(self.blockchain.last_hash, transaction, now);
        let block_timestamp = block.timestamp;

        self.balances.add_payment(&payment);

        let height = self.blockchain.add_block(block)?;

        self.transactions_by_hash.insert(transaction_hash, height);
        self.transactions_by_height.push_back(TransactionInfo {
            block_timestamp,
            transaction_hash,
        });

        let to_trim = if self.balances.store.len()
            >= self.maximum_number_of_accounts + self.accounts_overflow_trim_quantity
        {
            self.balances
                .select_accounts_to_trim(self.accounts_overflow_trim_quantity)
        } else {
            vec![]
        };

        for (balance, account) in to_trim {
            let operation = Operation::Burn {
                from: account,
                amount: balance,
            };
            self.balances.add_payment(&operation);
            self.blockchain
                .add_block(Block::new_from_transaction(
                    self.blockchain.last_hash,
                    Transaction {
                        operation,
                        memo: Memo::default(),
                        created_at_time: now,
                    },
                    now,
                ))
                .unwrap();
        }

        Ok((height, self.blockchain.last_hash.unwrap()))
    }

    /// Remove transactions older than `transaction_window`.
    fn purge_old_transactions(&mut self, now: TimeStamp) {
        while let Some(TransactionInfo {
            block_timestamp,
            transaction_hash,
        }) = self.transactions_by_height.front()
        {
            if *block_timestamp + self.transaction_window > now {
                // Stop at a sufficiently recent block.
                break;
            }
            let removed = self.transactions_by_hash.remove(transaction_hash);
            assert!(removed.is_some());

            // After 24 hours we don't need to store notification state because it isn't
            // accessible. We don't inspect the result because we don't care whether a
            // notification at this block height was made or not.
            match removed {
                Some(bh) => self.blocks_notified.remove(bh),
                None => None,
            };
            self.transactions_by_height.pop_front();
        }
    }

    /// This adds a pre created block to the ledger. This should only be used
    /// during canister migration or upgrade
    pub fn add_block(&mut self, block: Block) -> Result<BlockHeight, String> {
        self.balances.add_payment(&block.transaction.operation);
        self.blockchain.add_block(block)
    }

    pub fn from_init(
        &mut self,
        initial_values: HashMap<AccountIdentifier, TOKENs>,
        minting_account: AccountIdentifier,
        timestamp: TimeStamp,
        transaction_window: Option<Duration>,
        send_whitelist: HashSet<CanisterId>,
    ) {
        self.balances.icpt_pool = TOKENs::MAX;
        self.minting_account_id = Some(minting_account);
        if let Some(t) = transaction_window {
            self.transaction_window = t;
        }

        for (to, amount) in initial_values.into_iter() {
            self.add_payment_with_timestamp(
                Memo::default(),
                Operation::Mint { to, amount },
                None,
                timestamp,
            )
            .expect(&format!("Creating account {:?} failed", to)[..]);
        }

        self.send_whitelist = send_whitelist;
    }

    pub fn change_notification_state(
        &mut self,
        height: BlockHeight,
        block_timestamp: TimeStamp,
        new_state: bool,
        now: TimeStamp,
    ) -> Result<(), String> {
        if block_timestamp + self.transaction_window <= now {
            return Err(format!(
                "You cannot send a notification for a transaction that is more than {} seconds old",
                self.transaction_window.as_secs(),
            ));
        }

        let is_notified = self.blocks_notified.get(height).is_some();

        match (is_notified, new_state) {
            (true, true) | (false, false) => {
                Err(format!("The notification state is already {}", is_notified))
            }
            (true, false) => {
                self.blocks_notified.remove(height);
                Ok(())
            }
            (false, true) => {
                self.blocks_notified.insert(height, ());
                Ok(())
            }
        }
    }

    pub fn find_block_in_archive(&self, block_height: u64) -> Option<CanisterId> {
        let index = self
            .blockchain
            .archive
            .try_read()
            .expect("Failed to get lock on archive")
            .as_ref()
            .expect("archiving not enabled")
            .index();
        let result = index.binary_search_by(|((from, to), _)| {
            // If within the range we've found the right node
            if *from <= block_height && block_height <= *to {
                std::cmp::Ordering::Equal
            } else if *from < block_height {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        });
        match result {
            Ok(i) => Some(index[i].1),
            Err(_) => None,
        }
    }

    pub fn remove_archived_blocks(&mut self, len: usize) {
        self.blockchain.remove_archived_blocks(len);
    }

    pub fn get_blocks_for_archiving(
        &self,
        trigger_threshold: usize,
        num_blocks: usize,
    ) -> VecDeque<EncodedBlock> {
        self.blockchain
            .get_blocks_for_archiving(trigger_threshold, num_blocks)
    }

    pub fn can_send(&self, principal_id: &PrincipalId) -> bool {
        principal_id.is_self_authenticating()
            || LEDGER
                .read()
                .unwrap()
                .send_whitelist
                .contains(&CanisterId::new(*principal_id).unwrap())
    }

    /// Check if it's allowed to notify this canister
    /// Currently we reuse whitelist for that
    pub fn can_be_notified(&self, canister_id: &CanisterId) -> bool {
        LEDGER.read().unwrap().send_whitelist.contains(canister_id)
    }

    pub fn transactions_by_hash_len(&self) -> usize {
        self.transactions_by_hash.len()
    }

    pub fn transactions_by_height_len(&self) -> usize {
        self.transactions_by_height.len()
    }
}

lazy_static! {
    pub static ref LEDGER: RwLock<Ledger> = RwLock::new(Ledger::default());
    // Maximum inter-canister message size in bytes
    pub static ref MAX_MESSAGE_SIZE_BYTES: RwLock<usize> = RwLock::new(1024 * 1024);
}

pub fn add_payment(
    memo: Memo,
    payment: Operation,
    created_at_time: Option<TimeStamp>,
) -> (BlockHeight, HashOf<EncodedBlock>) {
    LEDGER
        .write()
        .unwrap()
        .add_payment(memo, payment, created_at_time)
        .expect("Transfer failed")
}

pub fn change_notification_state(
    height: BlockHeight,
    block_timestamp: TimeStamp,
    new_state: bool,
) -> Result<(), String> {
    LEDGER.write().unwrap().change_notification_state(
        height,
        block_timestamp,
        new_state,
        now().into(),
    )
}

// This is how we pass arguments to 'init' in main.rs
#[derive(Serialize, Deserialize, CandidType, Clone, Debug, PartialEq, Eq)]
pub struct LedgerCanisterInitPayload {
    pub minting_account: AccountIdentifier,
    pub initial_values: HashMap<AccountIdentifier, TOKENs>,
    pub max_message_size_bytes: Option<usize>,
    pub transaction_window: Option<Duration>,
    pub archive_options: Option<ArchiveOptions>,
    pub send_whitelist: HashSet<CanisterId>,
}

impl LedgerCanisterInitPayload {
    pub fn new(
        minting_account: AccountIdentifier,
        initial_values: HashMap<AccountIdentifier, TOKENs>,
        archive_options: Option<ArchiveOptions>,
        max_message_size_bytes: Option<usize>,
        transaction_window: Option<Duration>,
        send_whitelist: HashSet<CanisterId>,
    ) -> Self {
        // verify ledger's invariant about the maximum amount
        let _can_sum = initial_values.values().fold(TOKENs::ZERO, |acc, x| {
            (acc + *x).expect("Summation overflowing?")
        });

        // Don't allow self-transfers of the minting canister
        assert!(initial_values.get(&minting_account).is_none());

        Self {
            minting_account,
            initial_values,
            max_message_size_bytes,
            transaction_window,
            archive_options,
            send_whitelist,
        }
    }
}