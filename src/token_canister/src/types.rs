use crate::account_identifier::AccountIdentifier;
use crate::ic_token::TOKENs;
use crate::TimeStamp;
use crate::HashOf;

use candid::CandidType;
use ic_crypto_sha::Sha256;

use serde::{
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