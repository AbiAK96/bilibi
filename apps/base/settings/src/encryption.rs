//! Native authenticated envelope; no Laravel adoption codec. Associated data
//! binds ciphertext to its key and scope so copying rows cannot reveal a secret.
use aes_gcm::{
    aead::{Aead, Generate, Payload},
    Aes256Gcm, KeyInit, Nonce,
};
use bilibi_database::{Error, Result};
use serde_json::{json, Value};

pub struct Encryption {
    cipher: Option<Aes256Gcm>,
}
impl Encryption {
    /// Exactly 64 hex characters (32 random bytes). Missing/invalid keys are
    /// inert until an encrypted value is accessed. No Debug implementation.
    pub fn from_hex(key: Option<&str>) -> Self {
        let cipher = key.and_then(|key| {
            if key.len() != 64 || !key.is_ascii() {
                return None;
            }
            let mut bytes = [0u8; 32];
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&key[index * 2..index * 2 + 2], 16).ok()?;
            }
            let cipher = Aes256Gcm::new_from_slice(&bytes).ok();
            bytes.fill(0);
            cipher
        });
        Self { cipher }
    }
    /// Whether a valid key was configured; encrypted reads and writes refuse otherwise.
    pub fn available(&self) -> bool {
        self.cipher.is_some()
    }
    pub fn encrypt(&self, value: &Value, identity: &[u8]) -> Result<Value> {
        let cipher = self.cipher.as_ref().ok_or(Error::Invalid)?;
        let nonce = <[u8; 12]>::try_generate().map_err(|_| Error::Internal)?;
        let mut plaintext = serde_json::to_vec(value).map_err(|_| Error::Invalid)?;
        let encrypted = cipher.encrypt(
            Nonce::try_from(nonce.as_slice())
                .map_err(|_| Error::Invalid)?
                .as_ref(),
            Payload {
                msg: &plaintext,
                aad: identity,
            },
        );
        plaintext.fill(0);
        let encrypted = encrypted.map_err(|_| Error::Internal)?;
        Ok(json!({"version":1,"nonce":nonce,"ciphertext":encrypted}))
    }
    pub fn decrypt(&self, value: &Value, identity: &[u8]) -> Result<Value> {
        let cipher = self.cipher.as_ref().ok_or(Error::Invalid)?;
        if value.get("version").and_then(Value::as_u64) != Some(1) {
            return Err(Error::Invalid);
        }
        let nonce: Vec<u8> =
            serde_json::from_value(value["nonce"].clone()).map_err(|_| Error::Invalid)?;
        let encrypted: Vec<u8> =
            serde_json::from_value(value["ciphertext"].clone()).map_err(|_| Error::Invalid)?;
        if nonce.len() != 12 {
            return Err(Error::Invalid);
        }
        let mut plaintext = cipher
            .decrypt(
                Nonce::try_from(nonce.as_slice())
                    .map_err(|_| Error::Invalid)?
                    .as_ref(),
                Payload {
                    msg: &encrypted,
                    aad: identity,
                },
            )
            .map_err(|_| Error::Invalid)?;
        let decoded = serde_json::from_slice(&plaintext).map_err(|_| Error::Invalid);
        plaintext.fill(0);
        decoded
    }
}
