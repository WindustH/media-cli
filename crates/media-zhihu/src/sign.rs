//! Signatures: the Aliyun OSS header signature used by image uploads.

use base64::Engine;
use hmac::{Hmac, Mac};
use sha1::Sha1;

/// `Authorization` value for a `PUT` of `object_key` into the `zhihu-pics` bucket
/// with an STS security token (OSS signature version 1).
pub fn oss_authorization(
  access_id: &str,
  access_key: &str,
  security_token: &str,
  content_type: &str,
  date: &str,
  object_key: &str,
) -> String {
  let to_sign = format!(
    "PUT\n\n{content_type}\n{date}\nx-oss-security-token:{security_token}\n/zhihu-pics/{object_key}"
  );
  let mut mac = Hmac::<Sha1>::new_from_slice(access_key.as_bytes()).expect("any key length");
  mac.update(to_sign.as_bytes());
  let signature = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
  format!("OSS {access_id}:{signature}")
}
