//! Test vectors for ICCSA signatures.
#![allow(clippy::unwrap_used)]
use strum_macros::EnumIter;

#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, EnumIter)]
pub enum TestVectorId {
    STABILITY_1,
}

pub struct TestVector {
    pub signature: Vec<u8>,
    pub canister_id: String,
    pub seed: Vec<u8>,
    pub delegation_pubkey: Vec<u8>,
    pub delegation_exp: u64,
    pub root_pubkey_der: Vec<u8>,
}

pub fn test_vec(id: TestVectorId) -> TestVector {
    match id {
        TestVectorId::STABILITY_1 => {
            // The test data for this vector comes from a real idp_service canister.
            TestVector {
                signature: b"\xd9\xd9\xf7\xa2kcertificateY\x01\x8b\xd9\xd9\xf7\xa2dtree\x83\x01\x83\x01\x83\x01\x83\x02Hcanister\x83\x01\x82\x04X \xcb\xd2\x96\x8a\xb7\xb38?\x8e\x1c\x81\xe6\xec(\x11\xb0\x87?!\xea;z\xd5\xa7i\xeb\x900\xce\x028n\x83\x02J\x00\x00\x00\x00\x00\x00\x00\x08\x01\x01\x83\x01\x83\x01\x83\x02Ncertified_data\x82\x03X \xf3\xe9\x0c[F\xe5\xed?\xca\x88H\xf2\xe7\x16Q\xd4C\x9aI\xa2R\xb8;A}\x06G\xf3 \x5c\x07s\x82\x04X \xd0\x8a\xa5\x8br\x01}\xd5\xe5\x14\x9a\xc2.F\xa7\x86\xb6RN\xc1\xd8uj\xcf\x88\xb9\xe3|\x14\xef\x8f\xe2\x82\x04X \xa2\xa8r\x0a\xbb\xe0\xbde\x0f\x04\xa6o{\xfa\xde\xf7F\xb4t\xef)\xd4\x1d\x12k\xd1\xea#6\x15\xad\x18\x82\x04X \x99\xe5\x9e\x17\xdc\xe9\xa1?\xef\xc9\x22\xd2\x0a\x90\xdb\x03ow5'\x88\xe9\x9di\x8b6@\xd4\xfa\x88w$\x82\x04X E%X\xea\x92\xc9Q\xba\xf2\xbf5\xb2\x94\x00^\xde\xb8\xc3\xbf\x94\xb6\xaf\x9bk\x92\x83\x9d\xd2h\xa7\xcaZ\x83\x01\x82\x04X \xb4\xac\xde@n\xf5\x95\xb7\xb3\xc3\xf1be\xb9\xb3e\xbe\xe3\x82\x94\xa1\xb7\xa5\x9a\x9dQ\xd6B*\xb9\x98y\x83\x02Dtime\x82\x03I\xd8\xfb\x9a\xaf\xea\x8c\x96\xbb\x16isignatureX0\xb1Y\xa3\xd6_\x08\x22y\xff?Q]\x0f\xe7\xe8XC\x02\xb3k\xcc\x9ci8xH=O\x1d\x07\xb3\x5ci\x1a\xc5\xdf\x09\xbf\x96C\xca\xfb\x22\xca\xbb0\x07ndtree\x83\x02Csig\x83\x02X 9\xe5\xb4\x83\x0dM\x9c\x14\xdbsh\xa9[e\xd5F>\xa3\xd0\x95 77#C\x0c\x03\xa5\xa4S\xb5\xdf\x83\x02X \x8f\x7f\x1d\x02\xee_\xf0\xcd?;]\xd1\xd8r\xd0\x04\xdd\xe7\xf9\x18{\xc19\xd2\x07\xab\x09\x1d\xbdU\xe2t\x82\x03@".to_vec(),
                canister_id: "qoctq-giaaa-aaaaa-aaaea-cai".to_string(),
                seed: b"10000".to_vec(),
                delegation_pubkey: b"MY PUBLIC KEY".to_vec(),
                delegation_exp: 1_650_114_196_974_266_000,
                root_pubkey_der: base64::decode(&"MIGCMB0GDSsGAQQBgtx8BQMBAgEGDCsGAQQBgtx8BQMCAQNhAJN9lndC9PmwG44m08nlPFolGoNYavxz8FS6wa7WDBsR56ZnfsCyYIXNwdOa1MjctQLtFPVK9EDR2CkHWx6fbnLeV+uyOEphXQs+Lzpq9FFlMt5xOipXRXpmtosKfTT4Tg==".to_string()).unwrap(),
            }
        }
    }
}