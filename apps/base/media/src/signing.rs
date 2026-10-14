//! Signed, expiring stream tokens: `v1.{tenant}.{expires}.{mac}`. The MAC binds
//! the asset id from the URL, the owning tenant and the expiry, so a token for
//! one asset or tenant is useless for another. Signing needs its own operator
//! key (`BILIBI_MEDIA_SIGNING_KEY`, 64 hex characters), kept apart from the
//! Settings encryption key. No Debug: the key must never reach a log.
use crate::{Error, Result};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

pub struct Signer {
    key: Option<Vec<u8>>,
}
pub struct Claims {
    pub tenant: i64,
}
impl Signer {
    pub fn from_hex(key: Option<&str>) -> Self {
        let key = key
            .filter(|key| key.len() == 64 && key.is_ascii())
            .and_then(|key| {
                (0..32)
                    .map(|index| u8::from_str_radix(&key[index * 2..index * 2 + 2], 16).ok())
                    .collect()
            });
        Self { key }
    }
    fn mac(&self, asset: i64, tenant: i64, expires: u64) -> Result<Hmac<Sha256>> {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(self.key.as_deref().ok_or(Error::SigningUnavailable)?)
                .map_err(|_| Error::SigningUnavailable)?;
        mac.update(format!("bilibi-media-stream-v1|{asset}|{tenant}|{expires}").as_bytes());
        Ok(mac)
    }
    pub fn sign(&self, asset: i64, tenant: i64, expires: u64) -> Result<String> {
        let tag = self.mac(asset, tenant, expires)?.finalize().into_bytes();
        let tag: String = tag.iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(format!("v1.{tenant}.{expires}.{tag}"))
    }
    /// Constant-time MAC check first; expiry is reported only for authentic tokens.
    pub fn verify(&self, asset: i64, token: &str, now: u64) -> Result<Claims> {
        let parts: Vec<_> = token.split('.').collect();
        let [version, tenant, expires, tag] = parts[..] else {
            return Err(Error::InvalidToken);
        };
        let tenant: i64 = tenant.parse().map_err(|_| Error::InvalidToken)?;
        let expires: u64 = expires.parse().map_err(|_| Error::InvalidToken)?;
        let tag = decode_hex(tag).ok_or(Error::InvalidToken)?;
        if version != "v1" || tenant <= 0 {
            return Err(Error::InvalidToken);
        }
        self.mac(asset, tenant, expires)?
            .verify_slice(&tag)
            .map_err(|_| Error::InvalidToken)?;
        if expires <= now {
            return Err(Error::ExpiredToken);
        }
        Ok(Claims { tenant })
    }
}
fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len() / 2)
        .map(|index| u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok())
        .collect()
}
