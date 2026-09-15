use std::error::Error;
use std::fmt;
use std::sync::Arc;
use zeroize::Zeroize;

/// Secret values deliberately have no Debug or Serialize implementation. The
/// frontend/domain models never receive this type. Owned bytes are erased when
/// released; callers should keep any necessary plaintext borrows short-lived.
pub struct SecretValue(Vec<u8>);

impl SecretValue {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialError {
    Unavailable,
}

impl fmt::Display for CredentialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str(
                "Windows secure credential storage is unavailable. Gmail credentials were not saved.",
            ),
        }
    }
}

impl Error for CredentialError {}

/// There is intentionally no file, SQLite, environment, or browser fallback.
pub trait SecureCredentialService: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<SecretValue>, CredentialError>;
    fn set(&self, key: &str, secret: &SecretValue) -> Result<(), CredentialError>;
    fn delete(&self, key: &str) -> Result<(), CredentialError>;
}

/// Uses only Windows Credential Manager. Explicit platform selection prevents
/// keyring's mock fallback from ever becoming the production credential store.
#[cfg(windows)]
#[derive(Default)]
pub struct WindowsCredentialService;

#[cfg(windows)]
impl WindowsCredentialService {
    fn entry(key: &str) -> Result<keyring::Entry, CredentialError> {
        let credential = keyring::windows::WinCredential::new_with_target(
            None,
            "com.mailview.desktop.gmail",
            key,
        )
        .map_err(|_| CredentialError::Unavailable)?;
        Ok(keyring::Entry::new_with_credential(Box::new(credential)))
    }
}

#[cfg(windows)]
impl SecureCredentialService for WindowsCredentialService {
    fn get(&self, key: &str) -> Result<Option<SecretValue>, CredentialError> {
        match Self::entry(key)?.get_secret() {
            Ok(secret) => Ok(Some(SecretValue::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(CredentialError::Unavailable),
        }
    }

    fn set(&self, key: &str, secret: &SecretValue) -> Result<(), CredentialError> {
        Self::entry(key)?
            .set_secret(secret.expose())
            .map_err(|_| CredentialError::Unavailable)
    }

    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(CredentialError::Unavailable),
        }
    }
}

pub fn platform_secure_store() -> Arc<dyn SecureCredentialService> {
    #[cfg(windows)]
    {
        Arc::new(WindowsCredentialService)
    }
    #[cfg(not(windows))]
    {
        Arc::new(UnavailableCredentialService)
    }
}

/// Test injection is compiled out of production; it is never a fallback.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct MemoryCredentialService {
    values: std::sync::Mutex<std::collections::HashMap<String, SecretValue>>,
}

#[cfg(test)]
impl SecureCredentialService for MemoryCredentialService {
    fn get(&self, key: &str) -> Result<Option<SecretValue>, CredentialError> {
        let values = self
            .values
            .lock()
            .map_err(|_| CredentialError::Unavailable)?;
        Ok(values
            .get(key)
            .map(|secret| SecretValue::new(secret.expose().to_vec())))
    }

    fn set(&self, key: &str, secret: &SecretValue) -> Result<(), CredentialError> {
        self.values
            .lock()
            .map_err(|_| CredentialError::Unavailable)?
            .insert(key.to_owned(), SecretValue::new(secret.expose().to_vec()));
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        self.values
            .lock()
            .map_err(|_| CredentialError::Unavailable)?
            .remove(key);
        Ok(())
    }
}

#[derive(Default)]
pub struct UnavailableCredentialService;

impl SecureCredentialService for UnavailableCredentialService {
    fn get(&self, _key: &str) -> Result<Option<SecretValue>, CredentialError> {
        Err(CredentialError::Unavailable)
    }

    fn set(&self, _key: &str, _secret: &SecretValue) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable)
    }

    fn delete(&self, _key: &str) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_unconfigured_operation_fails_explicitly_and_redacts_inputs() {
        let store = UnavailableCredentialService;
        let secret = SecretValue::new(b"fictional-test-value".to_vec());
        assert!(matches!(
            store.get("test-key"),
            Err(CredentialError::Unavailable)
        ));
        assert_eq!(
            store.set("test-key", &secret),
            Err(CredentialError::Unavailable)
        );
        assert_eq!(store.delete("test-key"), Err(CredentialError::Unavailable));
        let message = CredentialError::Unavailable.to_string();
        assert!(!message.contains("test-key"));
        assert!(!message.contains("fictional-test-value"));
    }

    #[test]
    fn injected_memory_store_round_trips_and_deletes_binary_secrets() {
        let store = MemoryCredentialService::default();
        assert!(store.get("key").unwrap().is_none());
        store
            .set("key", &SecretValue::new(vec![0, 255, 1]))
            .unwrap();
        assert_eq!(store.get("key").unwrap().unwrap().expose(), [0, 255, 1]);
        store.delete("key").unwrap();
        store.delete("key").unwrap();
        assert!(store.get("key").unwrap().is_none());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Opt-in: writes a uniquely named fictional credential to Windows Credential Manager, then deletes it."]
    fn windows_credential_manager_round_trip() {
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = WindowsCredentialService.delete(&self.0);
            }
        }
        let key = Cleanup(format!("opt-in-smoke-{}", uuid::Uuid::new_v4()));
        for length in [39, 800, 2560] {
            let value = SecretValue::new(format!("AQ.{}", "f".repeat(length - 3)).into_bytes());
            WindowsCredentialService.set(&key.0, &value).unwrap();
            // Constructing another entry proves the complete value, including
            // long authorization keys, came from the OS store without truncation.
            let read = WindowsCredentialService.get(&key.0).unwrap().unwrap();
            assert_eq!(read.expose(), value.expose());
        }
        WindowsCredentialService.delete(&key.0).unwrap();
        assert!(WindowsCredentialService.get(&key.0).unwrap().is_none());
    }
}
