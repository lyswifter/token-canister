use crate::AccountIdentifier;
use crate::archive;

use crate::{LEDGER, TOKENs};
use crate::{MAX_MESSAGE_SIZE_BYTES, TRANSACTION_FEE, MIN_BURN_AMOUNT};
use crate::{Memo, Operation, TimeStamp, HashOf, BlockHeight, EncodedBlock, Subaccount};

use crate::add_payment;

use ic_types::CanisterId;
use ic_cdk_macros::*;

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use std::sync::Arc;
use std::time::Duration;

use dfn_core::api::{set_certified_data, caller};

use crate::print;


// Initialize the ledger canister
///
/// # Arguments
///
/// * `minting_account` -  The minting canister is given 2^64 - 1 tokens and it
///   then transfers tokens to addresses specified in the initial state.
///   Currently this is the only way to create tokens.
/// * `initial_values` - The list of accounts that will get balances at genesis.
///   This balances are paid out from the minting canister using 'Send'
///   transfers.
/// * `archive_canister` - The canister that manages the store of old blocks.
/// * `max_message_size_bytes` - The maximum message size that this subnet
///   supports. This is used for egressing block to the archive canister.
/// 
#[init]
fn init(
    minting_account: AccountIdentifier,
    initial_values: HashMap<AccountIdentifier, TOKENs>,
    max_message_size_bytes: Option<usize>,
    transaction_window: Option<Duration>,
    archive_options: Option<archive::ArchiveOptions>,
    send_whitelist: HashSet<CanisterId>,
) {
    print(format!(
        "[ledger] init(): minting account is {}",
        minting_account
    ));
    LEDGER.write().unwrap().from_init(
        initial_values,
        minting_account,
        dfn_core::api::now().into(),
        transaction_window,
        send_whitelist,
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

    if let Some(archive_options) = archive_options {
        LEDGER.write().unwrap().blockchain.archive =
            Arc::new(RwLock::new(Some(archive::Archive::new(archive_options))))
    }
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
// #[update]
async fn send(
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
    archive_blocks().await;
    height
}

/// Upon reaching a `trigger_threshold` we will archive `num_blocks`.
/// This really should be an action on the ledger canister, but since we don't
/// want to hold a mutable lock on the whole ledger while we're archiving, we
/// split this method up into the parts that require async (this function) and
/// the parts that require a lock (Ledger::get_blocks_for_archiving).
async fn archive_blocks() {
    let ledger_guard = LEDGER.try_read().expect("Failed to get ledger read lock");
    let archive_arc = ledger_guard.blockchain.archive.clone();
    let mut archive_guard = match archive_arc.try_write() {
        Ok(g) => g,
        Err(_) => {
            print("Ledger is currently archiving. Skipping archive_blocks()");
            return;
        }
    };
    if archive_guard.is_none() {
        return; // Archiving not enabled
    }
    let archive = archive_guard.as_mut().unwrap();

    let blocks_to_archive = ledger_guard
        .get_blocks_for_archiving(archive.trigger_threshold, archive.num_blocks_to_archive);
    if blocks_to_archive.is_empty() {
        return;
    }

    drop(ledger_guard); // Drop the lock on the ledger

    let num_blocks = blocks_to_archive.len();
    print(format!("[ledger] archiving {} blocks", num_blocks,));

    let max_msg_size = *MAX_MESSAGE_SIZE_BYTES.read().unwrap();
    let res = archive
        .send_blocks_to_archive(blocks_to_archive, max_msg_size)
        .await;

    let mut ledger = LEDGER.try_write().expect("Failed to get ledger write lock");
    match res {
        Ok(num_sent_blocks) => ledger.remove_archived_blocks(num_sent_blocks),
        Err((num_sent_blocks, archive::FailedToArchiveBlocks(err))) => {
            ledger.remove_archived_blocks(num_sent_blocks);
            print(format!(
                "[ledger] Archiving failed. Archived {} out of {} blocks. Error {}",
                num_sent_blocks, num_blocks, err
            ));
        }
    }
}