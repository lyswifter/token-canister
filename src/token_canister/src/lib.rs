use candid::CandidType;
use ic_types::{CanisterId, PrincipalId};
use intmap::IntMap;
use lazy_static::lazy_static;
use phantom_newtype::Id;
use serde::{
    de::{Deserializer, MapAccess, Visitor},
    ser::SerializeMap,
    Deserialize, Serialize, Serializer,
};
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::convert::TryInto;
use std::fmt;
use std::hash::Hash;
use std::marker::PhantomData;
use std::str::FromStr;
use std::sync::RwLock;
use std::time::Duration;

pub mod account_identifier;
pub mod ic_token;
pub mod ic_block;
pub mod interface;
pub mod hashof;
pub mod types;

#[path = "../gen/ic_ledger.pb.v1.rs"]
#[rustfmt::skip]
pub mod protobuf;
pub mod timestamp;
pub mod validate_endpoints;

use dfn_core::api::now;
pub use account_identifier::{AccountIdentifier, Subaccount};
pub use ic_token::{TOKENs, DECIMAL_PLACES, TOKEN_SUBDIVIDABLE_BY, MIN_BURN_AMOUNT, TRANSACTION_FEE};
pub use ic_block::{ Block, Blockchain, EncodedBlock, BlockHeight, get_blocks };
pub use protobuf::TimeStamp;
pub use types::{ Operation, Transaction, Memo};
pub use hashof::HashOf;

// Helper to print messages in magenta
pub fn print<S: std::convert::AsRef<str>>(s: S)
where
    yansi::Paint<S>: std::string::ToString,
{
    dfn_core::api::print(yansi::Paint::magenta(s).to_string());
}

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
            or equal to TOKENs::MAX, yet subtracting it lead to the following error: {}",
                e
            )
        })
    }
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


fn serialize_int_map<S>(im: &IntMap<()>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut map = serializer.serialize_map(Some(im.len()))?;
    for (k, v) in im.iter() {
        map.serialize_entry(k, v)?;
    }
    map.end()
}

struct IntMapVisitor<V> {
    marker: PhantomData<fn() -> IntMap<V>>,
}

impl<V> IntMapVisitor<V> {
    fn new() -> Self {
        IntMapVisitor {
            marker: PhantomData,
        }
    }
}

impl<'de, V> Visitor<'de> for IntMapVisitor<V>
where
    V: Deserialize<'de>,
{
    type Value = IntMap<V>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a very special map")
    }

    fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut map = IntMap::with_capacity(access.size_hint().unwrap_or(0));

        while let Some((key, value)) = access.next_entry()? {
            map.insert(key, value);
        }

        Ok(map)
    }
}

fn deserialize_int_map<'de, D>(deserializer: D) -> Result<IntMap<()>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_map(IntMapVisitor::new())
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Ledger {
    pub symbol: String,
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
    // Used to prevent non-whitelisted canisters from sending tokens
    // send_whitelist: HashSet<CanisterId>,
}

#[derive(Serialize, Deserialize, Debug)]
struct TransactionInfo {
    block_timestamp: TimeStamp,
    transaction_hash: HashOf<Transaction>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            symbol: "".to_string(),
            balances: LedgerBalances::default(),
            blockchain: Blockchain::default(),
            maximum_number_of_accounts: 50_000_000,
            accounts_overflow_trim_quantity: 100_000,
            minting_account_id: None,
            blocks_notified: IntMap::new(),
            transaction_window: Duration::from_secs(24 * 60 * 60),
            transactions_by_hash: BTreeMap::new(),
            transactions_by_height: VecDeque::new(),
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
        symbol: String,
        initial_values: HashMap<AccountIdentifier, TOKENs>,
        minting_account: AccountIdentifier,
        timestamp: TimeStamp,
        transaction_window: Option<Duration>,
    ) {
        self.symbol = symbol;
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
        !principal_id.is_anonymous()
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
}

impl LedgerCanisterInitPayload {
    pub fn new(
        minting_account: AccountIdentifier,
        initial_values: HashMap<AccountIdentifier, TOKENs>,
        max_message_size_bytes: Option<usize>,
        transaction_window: Option<Duration>,
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
        }
    }
}

/// Argument taken by the send endpoint
#[derive(Serialize, Deserialize, CandidType, Clone, Hash, Debug, PartialEq, Eq)]
pub struct SendArgs {
    pub memo: Memo,
    pub amount: TOKENs,
    pub fee: TOKENs,
    pub from_subaccount: Option<Subaccount>,
    pub to: AccountIdentifier,
    pub created_at_time: Option<TimeStamp>,
}

/// Struct sent by the ledger canister when it notifies a recipient of a payment
#[derive(Serialize, Deserialize, CandidType, Clone, Hash, Debug, PartialEq, Eq)]
pub struct TransactionNotification {
    pub from: PrincipalId,
    pub from_subaccount: Option<Subaccount>,
    pub to: CanisterId,
    pub to_subaccount: Option<Subaccount>,
    pub block_height: BlockHeight,
    pub amount: TOKENs,
    pub memo: Memo,
}

/// Argument taken by the notification endpoint
#[derive(Serialize, Deserialize, CandidType, Clone, Hash, Debug, PartialEq, Eq)]
pub struct NotifyCanisterArgs {
    pub block_height: BlockHeight,
    pub max_fee: TOKENs,
    pub from_subaccount: Option<Subaccount>,
    pub to_canister: CanisterId,
    pub to_subaccount: Option<Subaccount>,
}

impl NotifyCanisterArgs {
    /// Construct a `notify` call to notify a canister about the
    /// transaction created by a previous `send` call. `block_height`
    /// is the index of the block returned by `send`.
    pub fn new_from_send(
        send_args: &SendArgs,
        block_height: BlockHeight,
        to_canister: CanisterId,
        to_subaccount: Option<Subaccount>,
    ) -> Result<Self, String> {
        if AccountIdentifier::new(to_canister.get(), to_subaccount) != send_args.to {
            Err("Account identifier does not match canister args".to_string())
        } else {
            Ok(NotifyCanisterArgs {
                block_height,
                max_fee: send_args.fee,
                from_subaccount: send_args.from_subaccount,
                to_canister,
                to_subaccount,
            })
        }
    }
}

/// Argument taken by the account_balance endpoint
#[derive(Serialize, Deserialize, CandidType, Clone, Hash, Debug, PartialEq, Eq)]
pub struct AccountBalanceArgs {
    pub account: AccountIdentifier,
}

impl AccountBalanceArgs {
    pub fn new(account: AccountIdentifier) -> Self {
        AccountBalanceArgs { account }
    }
}

/// Argument taken by the total_supply endpoint
///
/// The reason it is a struct is so that it can be extended -- e.g., to be able
/// to query past values. Requiring 1 candid value instead of zero is a
/// non-backward compatible change. But adding optional fields to a struct taken
/// as input is backward-compatible.
#[derive(Serialize, Deserialize, CandidType, Clone, Hash, Debug, PartialEq, Eq)]
pub struct TotalSupplyArgs {}

#[derive(CandidType, Deserialize)]
pub enum CyclesResponse {
    CanisterCreated(CanisterId),
    // Silly requirement by the candid derivation
    ToppedUp(()),
    Refunded(String, Option<BlockHeight>),
}
