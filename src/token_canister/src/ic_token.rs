use candid::CandidType;
use core::ops::{Add, AddAssign, Sub, SubAssign};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(
    Serialize,
    Deserialize,
    CandidType,
    Clone,
    Copy,
    Hash,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
)]
pub struct TOKENs {
    /// Number of 10^-18 token.
    /// Named because the equivalent part of a Bitcoin is called a Satoshi
    e8s: u64,
}

pub const DECIMAL_PLACES: u32 = 18;
/// How many times can ICPs be divided
pub const TOKEN_SUBDIVIDABLE_BY: u64 = 100_000_000_000_000_000;

pub const TRANSACTION_FEE: TOKENs = TOKENs { e8s: 10_000 };
pub const MIN_BURN_AMOUNT: TOKENs = TRANSACTION_FEE;

impl TOKENs {

    pub const MAX: Self = TOKENs { e8s: u64::MAX };

    /// Construct a new instance of TOKENs.
    /// This function will not allow you use more than 1 TOKENs worth of E8s.
    pub fn new(tokens: u64, e8s: u64) -> Result<Self, String> {
        static CONSTRUCTION_FAILED: &str =
            "Constructing TOKEN failed because the underlying u64 overflowed";

        let token_part = tokens
            .checked_mul(TOKEN_SUBDIVIDABLE_BY)
            .ok_or_else(|| CONSTRUCTION_FAILED.to_string())?;
        if e8s >= TOKEN_SUBDIVIDABLE_BY {
            return Err(format!(
                "You've added too many E8s, make sure there are less than {}",
                TOKEN_SUBDIVIDABLE_BY
            ));
        }
        let e8s = token_part
            .checked_add(e8s)
            .ok_or_else(|| CONSTRUCTION_FAILED.to_string())?;
        Ok(Self { e8s })
    }

    pub const ZERO: Self = TOKENs { e8s: 0 };

    pub fn from_tokens(usdt: u64) -> Result<Self, String> {
        Self::new(usdt, 0)
    }

    pub const fn from_e8s(e8s: u64) -> Self {
        TOKENs { e8s }
    }

    pub fn get_tokens(self) -> u64 {
        self.e8s / TOKEN_SUBDIVIDABLE_BY
    }

    pub const fn get_e8s(self) -> u64 {
        self.e8s
    }

    pub fn get_remainder_e8s(self) -> u64 {
        self.e8s % TOKEN_SUBDIVIDABLE_BY
    }

    pub fn unpack(self) -> (u64, u64) {
        (self.get_tokens(), self.get_remainder_e8s())
    }
}

impl Add for TOKENs {
    type Output = Result<Self, String>;

    fn add(self, other: Self) -> Self::Output {
        let e8s = self.e8s.checked_add(other.e8s).ok_or_else(|| {
            format!(
                "Add TOKEN {} + {} failed because the underlying u64 overflowed",
                self.e8s, other.e8s
            )
        })?;
        Ok(Self { e8s })
    }
}

impl AddAssign for TOKENs {
    fn add_assign(&mut self, other: Self) {
        *self = (*self + other).expect("+= panicked");
    }
}

impl Sub for TOKENs {
    type Output = Result<Self, String>;

    fn sub(self, other: Self) -> Self::Output  {
        let e8s = self.e8s.checked_sub(other.e8s).ok_or_else(|| {
            format!(
                "Subtracting TOKEN {} - {} failed because the underlying u64 underflowed",
                self.e8s, other.e8s
            )
        })?;
        Ok(Self { e8s })
    }
}

impl SubAssign for TOKENs {
    fn sub_assign(&mut self, other: Self) {
        *self = (*self - other).expect("-= panicked");
    }
}

impl fmt::Display for TOKENs {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}.{:08} TOKEN",
            self.get_tokens(),
            self.get_remainder_e8s()
        )
    }
}