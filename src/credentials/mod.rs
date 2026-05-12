//! Credential storage for platform tokens.
//!
//! Supports OS keyring (primary) with env var fallback.
//! Tokens are keyed by `"{platform}:{host}:{username}"`.

use thiserror::Error;

const SERVICE_NAME: &str = "code-review";

#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("Credential not found for {0}")]
    NotFound(String),

    #[error("Keyring error: {0}")]
    Keyring(String),

    #[error("Storage not available: {0}")]
    Unavailable(String),
}

/// Trait for credential storage backends.
pub trait CredentialStore {
    /// Store a token for the given key.
    fn store(&self, key: &str, token: &str) -> Result<(), CredentialError>;

    /// Retrieve a token by key.
    fn get(&self, key: &str) -> Result<String, CredentialError>;

    /// Delete a stored token.
    fn delete(&self, key: &str) -> Result<(), CredentialError>;

    /// Check if a credential exists.
    fn exists(&self, key: &str) -> bool {
        self.get(key).is_ok()
    }
}

/// Build a credential key from platform components.
/// Format: `{platform}:{host}:{username}`
pub fn credential_key(platform: &str, host: &str, username: &str) -> String {
    format!("{platform}:{host}:{username}")
}

/// In-memory credential store for testing.
#[derive(Default)]
pub struct MemoryStore {
    entries: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl CredentialStore for MemoryStore {
    fn store(&self, key: &str, token: &str) -> Result<(), CredentialError> {
        self.entries
            .lock()
            .unwrap()
            .insert(key.to_string(), token.to_string());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<String, CredentialError> {
        self.entries
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .ok_or_else(|| CredentialError::NotFound(key.to_string()))
    }

    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        self.entries.lock().unwrap().remove(key);
        Ok(())
    }
}

/// Environment variable credential store — checks `GITHUB_TOKEN`, `GITLAB_TOKEN`, etc.
pub struct EnvVarStore;

impl CredentialStore for EnvVarStore {
    fn store(&self, _key: &str, _token: &str) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable(
            "Cannot write to environment variables".into(),
        ))
    }

    fn get(&self, key: &str) -> Result<String, CredentialError> {
        // Keys are formatted as "platform:host:username"
        let env_name = if key.starts_with("github:") {
            "GITHUB_TOKEN"
        } else if key.starts_with("gitlab:") {
            "GITLAB_TOKEN"
        } else {
            return Err(CredentialError::NotFound(key.to_string()));
        };

        std::env::var(env_name).map_err(|_| CredentialError::NotFound(key.to_string()))
    }

    fn delete(&self, _key: &str) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable(
            "Cannot delete environment variables".into(),
        ))
    }
}

/// Initialize the platform-specific keyring backend.
/// Must be called once before any KeyringStore operations.
/// Silently does nothing if the platform store can't be initialized.
pub fn init_keyring() {
    #[cfg(target_os = "macos")]
    {
        if let Ok(store) = apple_native_keyring_store::keychain::Store::new() {
            keyring_core::set_default_store(store);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(store) = windows_native_keyring_store::Store::new() {
            keyring_core::set_default_store(store);
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(store) = zbus_secret_service_keyring_store::Store::new() {
            keyring_core::set_default_store(store);
        }
    }
}

/// Keyring-backed credential store using the OS keychain.
///
/// Call `init_keyring()` before first use.
pub struct KeyringStore;

impl CredentialStore for KeyringStore {
    fn store(&self, key: &str, token: &str) -> Result<(), CredentialError> {
        let entry = keyring_core::Entry::new(SERVICE_NAME, key)
            .map_err(|e| CredentialError::Keyring(e.to_string()))?;
        entry
            .set_secret(token.as_bytes())
            .map_err(|e| CredentialError::Keyring(e.to_string()))
    }

    fn get(&self, key: &str) -> Result<String, CredentialError> {
        let entry = keyring_core::Entry::new(SERVICE_NAME, key)
            .map_err(|e| CredentialError::Keyring(e.to_string()))?;
        let secret = entry
            .get_secret()
            .map_err(|e| CredentialError::Keyring(e.to_string()))?;
        String::from_utf8(secret).map_err(|e| CredentialError::Keyring(e.to_string()))
    }

    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        let entry = keyring_core::Entry::new(SERVICE_NAME, key)
            .map_err(|e| CredentialError::Keyring(e.to_string()))?;
        entry
            .delete_credential()
            .map_err(|e| CredentialError::Keyring(e.to_string()))
    }
}

/// Try keyring first, fall back to env vars.
pub struct FallbackStore {
    keyring: KeyringStore,
    env: EnvVarStore,
}

impl FallbackStore {
    pub fn new() -> Self {
        init_keyring();
        Self {
            keyring: KeyringStore,
            env: EnvVarStore,
        }
    }
}

impl Default for FallbackStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialStore for FallbackStore {
    fn store(&self, key: &str, token: &str) -> Result<(), CredentialError> {
        self.keyring.store(key, token)
    }

    fn get(&self, key: &str) -> Result<String, CredentialError> {
        self.keyring.get(key).or_else(|_| self.env.get(key))
    }

    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        self.keyring.delete(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests use MemoryStore — no real keyring access needed

    #[test]
    fn memory_store_roundtrip() {
        let store = MemoryStore::new();
        store.store("github.com:alice", "ghp_xxx").unwrap();

        let token = store.get("github.com:alice").unwrap();
        assert_eq!(token, "ghp_xxx");
    }

    #[test]
    fn memory_store_not_found() {
        let store = MemoryStore::new();
        let result = store.get("nonexistent");
        assert!(matches!(result, Err(CredentialError::NotFound(_))));
    }

    #[test]
    fn memory_store_delete() {
        let store = MemoryStore::new();
        store.store("key", "value").unwrap();
        store.delete("key").unwrap();
        assert!(!store.exists("key"));
    }

    #[test]
    fn memory_store_exists() {
        let store = MemoryStore::new();
        assert!(!store.exists("key"));
        store.store("key", "val").unwrap();
        assert!(store.exists("key"));
    }

    #[test]
    fn memory_store_overwrite() {
        let store = MemoryStore::new();
        store.store("key", "old").unwrap();
        store.store("key", "new").unwrap();
        assert_eq!(store.get("key").unwrap(), "new");
    }

    #[test]
    fn credential_key_format() {
        assert_eq!(
            credential_key("github", "github.com", "alice"),
            "github:github.com:alice"
        );
        assert_eq!(
            credential_key("gitlab", "gitlab.example.com", "bob"),
            "gitlab:gitlab.example.com:bob"
        );
    }

    #[test]
    fn env_var_store_not_found() {
        let store = EnvVarStore;
        let result = store.get("unknown:host:user");
        assert!(matches!(result, Err(CredentialError::NotFound(_))));
    }

    #[test]
    fn env_var_store_cannot_write() {
        let store = EnvVarStore;
        let result = store.store("key", "val");
        assert!(matches!(result, Err(CredentialError::Unavailable(_))));
    }

    #[test]
    fn delete_nonexistent_key_succeeds() {
        let store = MemoryStore::new();
        // Deleting a key that doesn't exist should not error
        store.delete("nonexistent").unwrap();
    }
}
