//! The one place passwords are hashed, checked and bounded. Argon2 is CPU-bound
//! and runs on the blocking pool; every path that cannot find an account still
//! pays for one verification so timing does not reveal which emails exist.
use argon2::{
    password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier},
    Argon2,
};
use bilibi_database::{Error, Result};

pub const MIN_LENGTH: usize = 8;
/// Bytes. Bounds Argon2's input; login already refuses anything longer.
pub const MAX_LENGTH: usize = 1024;

/// A valid hash of a random secret, verified against when no account matched.
const UNKNOWN_ACCOUNT_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRmb3J0ZXN0aW5n$0A3fpxjnP4JPN0QSxhKQJNfBQYkxMvEQvEhu0Zp5H8c";

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PolicyViolation {
    #[error("should be at least 8 character(s)")]
    TooShort,
    #[error("should be at most 1024 byte(s)")]
    TooLong,
}

pub fn check_policy(password: &str) -> std::result::Result<(), PolicyViolation> {
    if password.chars().count() < MIN_LENGTH {
        Err(PolicyViolation::TooShort)
    } else if password.len() > MAX_LENGTH {
        Err(PolicyViolation::TooLong)
    } else {
        Ok(())
    }
}

pub fn hash_blocking(password: &str) -> Result<String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|_| Error::Internal)
}
pub async fn hash(password: &str) -> Result<String> {
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || hash_blocking(&password))
        .await
        .map_err(|_| Error::Internal)?
}

/// `None` means no usable account: the dummy hash is checked and the answer is false.
pub async fn verify(password: &str, stored: Option<&str>) -> Result<bool> {
    let known = stored.is_some();
    let stored = stored.unwrap_or(UNKNOWN_ACCOUNT_HASH).to_owned();
    let password = password.to_owned();
    let matched = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&stored).ok().is_some_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .map_err(|_| Error::Internal)?;
    Ok(known && matched)
}
