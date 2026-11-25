//! The bearer token Tracket sends to the local API. Made once from the OS's random source and kept
//! in the system keychain (Windows Credential Manager), never in a file.

use keyring::Entry;

use crate::error::{AppError, Result};

const SERVICE: &str = "sprocket";
const USER: &str = "api-token";

fn entry() -> Result<Entry> {
    Entry::new(SERVICE, USER).map_err(|e| AppError::new("keychain", e.to_string()))
}

fn generate() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| AppError::new("keychain", e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The token, created and stored on first use.
pub fn load_or_create() -> Result<String> {
    let entry = entry()?;
    match entry.get_password() {
        Ok(token) => Ok(token),
        Err(keyring::Error::NoEntry) => {
            let token = generate()?;
            entry
                .set_password(&token)
                .map_err(|e| AppError::new("keychain", e.to_string()))?;
            Ok(token)
        }
        Err(e) => Err(AppError::new("keychain", e.to_string())),
    }
}

/// Compares in constant time, so the answer's timing says nothing about the token.
pub fn matches(expected: &str, given: &str) -> bool {
    let (a, b) = (expected.as_bytes(), given.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::{generate, matches};

    #[test]
    fn tokens_are_64_hex_characters_and_differ() {
        let (a, b) = (generate().unwrap(), generate().unwrap());
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn matches_only_the_same_token() {
        assert!(matches("abc", "abc"));
        assert!(!matches("abc", "abd"));
        assert!(!matches("abc", "ab"));
        assert!(!matches("abc", ""));
    }
}
