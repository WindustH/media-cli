//! xhshow's `CryptoConfig`, with the overrides `xhs_cli/signing.py` applies
//! (macOS platform, web SDK 4.2.6). Only the constants the signer reads are kept.

/// `x0` / `x1` of the signature templates.
pub const SDK_VERSION: &str = "4.2.6";
pub const APP_ID: &str = "xhs-pc-web";
pub const PLATFORM: &str = "macOS";
/// `x4` of the x-s-common template.
pub const WEB_BUILD: &str = "4.86.0";

pub const CUSTOM_BASE64_ALPHABET: &str =
  "ZmserbBoHQtNP+wOcza/LpngG8yJq42KWYj0DSfdikx3VT16IlUAFM97hECvuRX5";
pub const X3_BASE64_ALPHABET: &str =
  "MfgqrsbcyzPQRStuvC7mn501HIJBo2DEFTKdeNOwxWXYZap89+/A4UVLhijkl63G";

/// XOR key applied to the first 144 payload bytes.
pub const HEX_KEY: &str = "71a302257793271ddd273bcee3e4b98d9d7935e1da33f5765e2ea8afb6dc77a51a499d23b67c20660025860cbf13d4540d92497f58686c574e508f46e1956344f39139bf4faf22a3eef120b79258145b2feb5193b6478669961298e79bedca646e1a693a926154a5a7a1bd1cf0dedb742f917a747a1e388b234f2277516db7116035439730fa61e9822a0eca7bff72d8";

pub const VERSION_BYTES: [u8; 4] = [121, 104, 96, 41];
pub const PAYLOAD_LENGTH: usize = 144;
pub const A1_LENGTH: usize = 52;
pub const APP_ID_LENGTH: usize = 10;
pub const MD5_XOR_LENGTH: usize = 8;
pub const A3_PREFIX: [u8; 4] = [2, 97, 51, 16];

/// Environment detection table (part 11 of the payload).
pub const ENV_TABLE: [u8; 15] = [
  115, 248, 83, 102, 103, 201, 181, 131, 99, 94, 4, 68, 250, 132, 21,
];
/// Environment checks of a normal browser.
pub const ENV_CHECKS_DEFAULT: [u8; 15] = [0, 1, 18, 1, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0];

/// Initial state of `custom_hash_v2`.
pub const HASH_IV: [u32; 4] = [1831565813, 461845907, 2246822507, 3266489909];

// Session simulation ranges (inclusive).
pub const SESSION_SEQUENCE_INIT: (u32, u32) = (15, 17);
pub const SESSION_SEQUENCE_STEP: (u32, u32) = (0, 1);
pub const SESSION_WINDOW_PROPS_INIT: (u32, u32) = (1000, 2000);
pub const SESSION_WINDOW_PROPS_STEP: (u32, u32) = (1, 10);

pub const X3_PREFIX: &str = "mns0301_";
pub const XYS_PREFIX: &str = "XYS_";
pub const XYW_PREFIX: &str = "XYW_";

pub const HEX_CHARS: &[u8] = b"abcdef0123456789";
pub const XRAY_TRACE_ID_SEQ_MAX: u64 = 8_388_607;
pub const XRAY_TRACE_ID_TIMESTAMP_SHIFT: u32 = 23;
pub const TRACE_ID_LENGTH: usize = 16;

/// RC4 key of the b1 fingerprint.
pub const B1_SECRET_KEY: &[u8] = b"xhswebmplfbt";

// Creator (XYW) signing, from `xhs_cli/creator_signing.py`.
pub const XYW_AES_KEY: &[u8; 16] = b"7cc4adla5ay0701v";
pub const XYW_AES_IV: &[u8; 16] = b"4uzjr7mbsibcaldp";
pub const XYW_CREATOR_ENV_FLAGS: &str = "0|0|0|1|0|0|1|0|0|0|1|0|0|0|0|1|0|0|0";
