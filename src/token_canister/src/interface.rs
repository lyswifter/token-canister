use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::AccountIdentifier;
use crate::protobuf;
use crate::{LEDGER, TOKENs};
use crate::{MAX_MESSAGE_SIZE_BYTES, TRANSACTION_FEE, MIN_BURN_AMOUNT};
use crate::{TimeStamp, HashOf, Subaccount, SendArgs, TransactionNotification, NotifyCanisterArgs};
use crate::{AccountBalanceArgs, TotalSupplyArgs};

use crate::types::{ Memo, Transaction, Operation};

use crate::ic_block::{TipOfChainRes, BlockRes, BlockArg, GetBlocksArgs, IterBlocksArgs, BlockHeight, EncodedBlock, Blockchain, iter_blocks, get_blocks};

use crate:: { change_notification_state};
use crate::add_payment;
use crate::print;

use dfn_candid::{candid, candid_one, CandidOne};

use on_wire::IntoWire;
use ic_types::CanisterId;
use ic_cdk_macros::*;

use dfn_protobuf::{protobuf, ProtoBuf};
use dfn_core::{
    api::{
        call_bytes_with_cleanup, call_with_cleanup, caller, data_certificate, set_certified_data,
        Funds,
    },
    endpoint::over_async_may_reject_explicit,
    over, over_async, over_init, printer, setup, stable, BytesS,
};


// Initialize the ledger canister
///
/// # Arguments
/// * `symbol` - The symbol name you specify to the token
/// * `minting_account` -  The minting canister is given 2^64 - 1 tokens and it
///   then transfers tokens to addresses specified in the initial state.
///   Currently this is the only way to create tokens.
/// * `initial_values` - The list of accounts that will get balances at genesis.
///   This balances are paid out from the minting canister using 'Send'
///   transfers.
/// * `archive_canister` - The canister that manages the store of old blocks.
/// * `max_message_size_bytes` - The maximum message size that this subnet
///   supports. This is used for egressing block to the archive canister.
// #[init]
fn init(
    symbol: String,
    minting_account: AccountIdentifier,
    initial_values: HashMap<AccountIdentifier, TOKENs>,
    max_message_size_bytes: Option<usize>,
    transaction_window: Option<Duration>,
) {
    print(format!(
        "[ledger] init(): minting account is {}",
        minting_account
    ));
    LEDGER.write().unwrap().from_init(
        symbol,
        initial_values,
        minting_account,
        dfn_core::api::now().into(),
        transaction_window,
    );
    match max_message_size_bytes {
        None => {
            print(format!(
                "[ledger] init(): using default maximum message size: {}",
                MAX_MESSAGE_SIZE_BYTES.read().unwrap()
            ));
        }
        Some(max_message_size_bytes) => {
            *MAX_MESSAGE_SIZE_BYTES.write().unwrap() = max_message_size_bytes;
            print(format!(
                "[ledger] init(): using maximum message size: {}",
                max_message_size_bytes
            ));
        }
    }
    set_certified_data(
        &LEDGER
            .read()
            .unwrap()
            .blockchain
            .last_hash
            .map(|h| h.into_bytes())
            .unwrap_or([0u8; 32]),
    );
}

fn add_payments(
    memo: Memo,
    operation: Operation,
    created_at_time: Option<TimeStamp>,
) -> (BlockHeight, HashOf<EncodedBlock>) {
    let (height, hash) = add_payment(memo, operation, created_at_time);
    set_certified_data(&hash.into_bytes());
    (height, hash)
}

/// This is the only operation that changes the state of the canister blocks and
/// balances after init. This creates a payment from the caller's account. It
/// returns the index of the resulting transaction
///
/// # Arguments
///
/// * `memo` -  A 8 byte "message" you can attach to transactions to help the
///   receiver disambiguate transactions
/// * `amount` - The number of ICPTs the recipient gets. The number of ICPTs
///   withdrawn is equal to the amount + the fee
/// * `fee` - The maximum fee that the sender is willing to pay. If the required
///   fee is greater than this the transaction will be rejected otherwise the
///   required fee will be paid. TODO(ROSETTA1-45): automatically pay a lower
///   fee if possible
/// * `from_subaccount` - The subaccount you want to draw funds from
/// * `to` - The account you want to send the funds to
/// * `to_subaccount` - The subaccount you want to send funds to
pub async fn send(
    memo: Memo,
    amount: TOKENs,
    fee: TOKENs,
    from_subaccount: Option<Subaccount>,
    to: AccountIdentifier,
    created_at_time: Option<TimeStamp>,
) -> BlockHeight {
    let caller_principal_id = caller();

    if !LEDGER.read().unwrap().can_send(&caller_principal_id) {
        panic!(
            "Sending from non-self-authenticating principal or non-whitelisted canister is not allowed: {}",
            caller_principal_id
        );
    }

    let from = AccountIdentifier::new(caller_principal_id, from_subaccount);
    let minting_acc = LEDGER
        .read()
        .unwrap()
        .minting_account_id
        .expect("Minting canister id not initialized");

    let transfer = if from == minting_acc {
        assert_eq!(fee, TOKENs::ZERO, "Fee for minting should be zero");
        assert_ne!(
            to, minting_acc,
            "It is illegal to mint to a minting_account"
        );
        Operation::Mint { to, amount }
    } else if to == minting_acc {
        assert_eq!(fee, TOKENs::ZERO, "Fee for burning should be zero");
        if amount < MIN_BURN_AMOUNT {
            panic!("Burns lower than {} are not allowed", MIN_BURN_AMOUNT);
        }
        Operation::Burn { from, amount }
    } else {
        if fee != TRANSACTION_FEE {
            panic!("Transaction fee should be {}", TRANSACTION_FEE);
        }
        Operation::Transfer {
            from,
            to,
            amount,
            fee,
        }
    };
    let (height, _) = add_payments(memo, transfer, created_at_time);
    // Don't put anything that could ever trap after this call or people using this
    // endpoint. If something did panic the payment would appear to fail, but would
    // actually succeed on chain.
    // archive_blocks().await;
    height
}

/// This gives you the index of the last block added to the chain
/// together with certification
fn tip_of_chain() -> TipOfChainRes {
    let last_block_idx = &LEDGER
        .read()
        .unwrap()
        .blockchain
        .chain_length()
        .checked_sub(1)
        .unwrap();
    let certification = data_certificate();
    TipOfChainRes {
        certification,
        tip_index: *last_block_idx,
    }
}

// This is going away and being replaced by getblocks
fn block(block_index: BlockHeight) -> Option<Result<EncodedBlock, CanisterId>> {
    let state = LEDGER.read().unwrap();
    // if block_index < state.blockchain.num_archived_blocks() {
        // The block we are looking for better be in the archive because it has
        // a height smaller than the number of blocks we've archived so far
        // let result = state
        //     .find_block_in_archive(block_index)
        //     .expect("block not found in the archive");
        // Some(Err(result))
    // Or the block may be in the ledger, or the block may not exist
    // } else {
        print(format!(
            "[ledger] Checking the ledger for block [{}]",
            block_index
        ));
        state.blockchain.get(block_index).cloned().map(Ok)
    // }
}

/// Get an account balance.
/// If the account does not exist it will return 0 ICPTs
fn account_balance(account: AccountIdentifier) -> TOKENs {
    LEDGER.read().unwrap().balances.account_balance(&account)
}

/// The total number of ICPTs not inside the minting canister
fn total_supply() -> TOKENs {
    LEDGER.read().unwrap().balances.total_supply()
}

/// Canister endpoints
#[update]
fn send_() {
    over_async(
        protobuf,
        |SendArgs {
             memo,
             amount,
             fee,
             from_subaccount,
             to,
             created_at_time,
         }| { send(memo, amount, fee, from_subaccount, to, created_at_time) },
    );
}

/// Do not use call this from code, this is only here so dfx has something to
/// call when making a payment. This will be changed in ways that are not
/// backwards compatible with previous interfaces.
///
/// I STRONGLY recommend that you use "send_pb" instead.
#[export_name = "canister_update send_dfx"]
fn send_dfx_() {
    over_async(
        candid_one,
        |SendArgs {
             memo,
             amount,
             fee,
             from_subaccount,
             to,
             created_at_time,
         }| { send(memo, amount, fee, from_subaccount, to, created_at_time) },
    );
}

#[export_name = "canister_query block_pb"]
fn block_() {
    over(protobuf, |BlockArg(height)| BlockRes(block(height)));
}

#[export_name = "canister_query tip_of_chain_pb"]
fn tip_of_chain_() {
    over(protobuf, |protobuf::TipOfChainRequest {}| tip_of_chain());
}

#[export_name = "canister_query account_balance_pb"]
fn account_balance_() {
    over(protobuf, |AccountBalanceArgs { account }| {
        account_balance(account)
    })
}

/// See caveats of use on send_dfx
#[export_name = "canister_query account_balance_dfx"]
fn account_balance_dfx_() {
    over(candid_one, |AccountBalanceArgs { account }| {
        account_balance(account)
    })
}

#[export_name = "canister_query total_supply_pb"]
fn total_supply_() {
    over(protobuf, |_: TotalSupplyArgs| total_supply())
}

/// Get multiple blocks by *offset into the container* (not BlockHeight) and
/// length. Note that this simply iterates the blocks available in the Ledger
/// without taking into account the archive. For example, if the ledger contains
/// blocks with heights [100, 199] then iter_blocks(0, 1) will return the block
/// with height 100.
#[export_name = "canister_query iter_blocks_pb"]
fn iter_blocks_() {
    over(protobuf, |IterBlocksArgs { start, length }| {
        let blocks = &LEDGER.read().unwrap().blockchain.blocks;
        iter_blocks(blocks, start, length)
    });
}

/// Get multiple blocks by BlockHeight and length. If the query is outside the
/// range stored in the Node the result is an error.
#[export_name = "canister_query get_blocks_pb"]
fn get_blocks_() {
    over(protobuf, |GetBlocksArgs { start, length }| {
        let blockchain: &Blockchain = &LEDGER.read().unwrap().blockchain;
        let start_offset = blockchain.num_archived_blocks();
        get_blocks(&blockchain.blocks, start_offset, start, length)
    });
}

#[export_name = "canister_post_upgrade"]
fn post_upgrade() {
    over_init(|_: BytesS| {
        let mut ledger = LEDGER.write().unwrap();
        *ledger = serde_cbor::from_reader(&mut stable::StableReader::new())
            .expect("Decoding stable memory failed");

        set_certified_data(
            &ledger
                .blockchain
                .last_hash
                .map(|h| h.into_bytes())
                .unwrap_or([0u8; 32]),
        );
    })
}

#[export_name = "canister_pre_upgrade"]
fn pre_upgrade() {
    use std::io::Write;

    setup::START.call_once(|| {
        printer::hook();
    });

    let ledger = LEDGER
        .read()
        // This should never happen, but it's better to be safe than sorry
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut writer = stable::StableWriter::new();
    serde_cbor::to_writer(&mut writer, &*ledger).unwrap();
    writer
        .flush()
        .expect("failed to flush stable memory writer");
}