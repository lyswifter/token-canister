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
pub struct USDTs {
    /// Number of 10^-18 USDT.
    /// Named because the equivalent part of a Bitcoin is called a Satoshi
    e8s: u64,
}

pub const DECIMAL_PLACES: u32 = 18;
/// How many times can ICPs be divided
pub const USDT_SUBDIVIDABLE_BY: u64 = 100_000_000_000_000_000;

pub const TRANSACTION_FEE: USDTs = USDTs { e8s: 10_000 };
pub const MIN_BURN_AMOUNT: USDTs = TRANSACTION_FEE;

impl USDTs {

    pub const MAX: Self = USDTs { e8s: u64::MAX };

    /// Construct a new instance of USDTs.
    /// This function will not allow you use more than 1 USDTs worth of E8s.
    pub fn new(usdts: u64, e8s: u64) -> Result<Self, String> {
        static CONSTRUCTION_FAILED: &str =
            "Constructing USDT failed because the underlying u64 overflowed";

        let usdt_part = usdts
            .checked_mul(USDT_SUBDIVIDABLE_BY)
            .ok_or_else(|| CONSTRUCTION_FAILED.to_string())?;
        if e8s >= USDT_SUBDIVIDABLE_BY {
            return Err(format!(
                "You've added too many E8s, make sure there are less than {}",
                USDT_SUBDIVIDABLE_BY
            ));
        }
        let e8s = usdt_part
            .checked_add(e8s)
            .ok_or_else(|| CONSTRUCTION_FAILED.to_string())?;
        Ok(Self { e8s })
    }

    pub const ZERO: Self = USDTs { e8s: 0 };

    pub fn from_usdts(usdt: u64) -> Result<Self, String> {
        Self::new(usdt, 0)
    }

    pub const fn from_e8s(e8s: u64) -> Self {
        USDTs { e8s }
    }

    pub fn get_usdts(self) -> u64 {
        self.e8s / USDT_SUBDIVIDABLE_BY
    }

    pub const fn get_e8s(self) -> u64 {
        self.e8s
    }

    pub fn get_remainder_e8s(self) -> u64 {
        self.e8s % USDT_SUBDIVIDABLE_BY
    }

    pub fn unpack(self) -> (u64, u64) {
        (self.get_usdts(), self.get_remainder_e8s())
    }
}

impl Add for USDTs {
    fn add(self, other: Self) -> Result<Self, String> {
        let e8s = self.e8s.checked_add(other.e8s).ok_or_else(|| {
            format!(
                "Add USDT {} + {} failed because the underlying u64 overflowed",
                self.e8s, other.e8s
            )
        })?;
        Ok(Self { e8s })
    }
}

impl AddAssign for USDTs {
    fn add_assign(&mut self, other: Self) {
        *self = (*self + other).expect("+= panicked");
    }
}

impl SubAssign for USDTs {
    fn sub_assign(&mut self, other: Self) {
        *self = (*self - other).expect("-= panicked");
    }
}

impl Display for USDTs {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}.{:08} USDT",
            self.get_usdts(),
            self.get_remainder_e8s()
        )
    }   
}