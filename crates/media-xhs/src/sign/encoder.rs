//! Base64 with the two substituted alphabets (xhshow `utils/encoder.py`):
//! standard padded Base64 whose letters are mapped onto another alphabet.

use base64::Engine;
use base64::alphabet::Alphabet;
use base64::engine::GeneralPurpose;
use base64::engine::general_purpose::PAD;

use super::config::{CUSTOM_BASE64_ALPHABET, X3_BASE64_ALPHABET};

const fn engine(alphabet: &str) -> GeneralPurpose {
  match Alphabet::new(alphabet) {
    Ok(a) => GeneralPurpose::new(&a, PAD),
    Err(_) => panic!("invalid base64 alphabet"),
  }
}

const CUSTOM: GeneralPurpose = engine(CUSTOM_BASE64_ALPHABET);
const X3: GeneralPurpose = engine(X3_BASE64_ALPHABET);

/// x-s / x-s-common / b1 encoding.
pub fn encode(data: impl AsRef<[u8]>) -> String {
  CUSTOM.encode(data)
}

/// Encoding of the `x3` field inside x-s.
pub fn encode_x3(data: impl AsRef<[u8]>) -> String {
  X3.encode(data)
}
