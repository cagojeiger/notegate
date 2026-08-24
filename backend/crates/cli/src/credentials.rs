use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use fd_lock::RwLock;
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};

const BUNDLE_VERSION: u8 = 1;
const INDEX_VERSION: u8 = 1;
const KEYRING_SERVICE: &str = "io.project-jelly.notegate-cli";

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub(crate) struct CredentialKey {
    pub(crate) issuer: String,
    pub(crate) client_id: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RefreshState {
    Ready,
    Uncertain,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct TokenBundle {
    version: u8,
    pub(crate) base_url: String,
    pub(crate) issuer: String,
    pub(crate) client_id: String,
    pub(crate) token_endpoint: String,
    pub(crate) revocation_endpoint: String,
    access_token: SecretValue,
    refresh_token: SecretValue,
    pub(crate) expires_at: u64,
    pub(crate) refresh_state: RefreshState,
}

impl fmt::Debug for TokenBundle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenBundle")
            .field("version", &self.version)
            .field("base_url", &self.base_url)
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("token_endpoint", &self.token_endpoint)
            .field("revocation_endpoint", &self.revocation_endpoint)
            .field("access_token", &self.access_token)
            .field("refresh_token", &self.refresh_token)
            .field("expires_at", &self.expires_at)
            .field("refresh_state", &self.refresh_state)
            .finish()
    }
}

impl TokenBundle {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        base_url: String,
        issuer: String,
        client_id: String,
        token_endpoint: String,
        revocation_endpoint: String,
        access_token: String,
        refresh_token: String,
        expires_at: u64,
    ) -> Self {
        Self {
            version: BUNDLE_VERSION,
            base_url,
            issuer,
            client_id,
            token_endpoint,
            revocation_endpoint,
            access_token: SecretValue::new(access_token),
            refresh_token: SecretValue::new(refresh_token),
            expires_at,
            refresh_state: RefreshState::Ready,
        }
    }

    pub(crate) fn key(&self) -> CredentialKey {
        CredentialKey {
            issuer: self.issuer.clone(),
            client_id: self.client_id.clone(),
        }
    }

    pub(crate) fn access_token(&self) -> SecretString {
        self.access_token.to_secret_string()
    }

    pub(crate) fn refresh_token(&self) -> &str {
        self.refresh_token.expose()
    }

    pub(crate) fn replace_tokens(
        &mut self,
        access_token: String,
        refresh_token: String,
        expires_at: u64,
    ) {
        self.access_token = SecretValue::new(access_token);
        self.refresh_token = SecretValue::new(refresh_token);
        self.expires_at = expires_at;
        self.refresh_state = RefreshState::Ready;
    }

    pub(crate) fn mark_refresh_uncertain(&mut self) {
        self.refresh_state = RefreshState::Uncertain;
    }

    fn validate(&self) -> Result<(), StoreError> {
        if self.version != BUNDLE_VERSION {
            return Err(StoreError::corrupt());
        }
        if [
            self.base_url.as_str(),
            self.issuer.as_str(),
            self.client_id.as_str(),
            self.token_endpoint.as_str(),
            self.revocation_endpoint.as_str(),
            self.access_token.expose(),
            self.refresh_token.expose(),
        ]
        .iter()
        .any(|value| value.is_empty())
        {
            return Err(StoreError::corrupt());
        }
        Ok(())
    }
}

#[derive(Clone)]
struct SecretValue(SecretString);

impl SecretValue {
    fn new(value: String) -> Self {
        Self(SecretString::from(value))
    }

    fn expose(&self) -> &str {
        self.0.expose_secret()
    }

    fn to_secret_string(&self) -> SecretString {
        SecretString::from(self.expose().to_owned())
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

impl Serialize for SecretValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.expose())
    }
}

impl<'de> Deserialize<'de> for SecretValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::new)
    }
}

#[derive(Debug)]
pub(crate) struct StoreError {
    kind: StoreErrorKind,
}

#[derive(Debug)]
enum StoreErrorKind {
    Unavailable,
    Corrupt,
    AlreadyExists,
    Missing,
    StateUnknown,
}

impl StoreError {
    pub(crate) fn unavailable() -> Self {
        Self {
            kind: StoreErrorKind::Unavailable,
        }
    }

    pub(crate) fn corrupt() -> Self {
        Self {
            kind: StoreErrorKind::Corrupt,
        }
    }

    pub(crate) fn already_exists() -> Self {
        Self {
            kind: StoreErrorKind::AlreadyExists,
        }
    }

    pub(crate) fn missing() -> Self {
        Self {
            kind: StoreErrorKind::Missing,
        }
    }

    fn state_unknown() -> Self {
        Self {
            kind: StoreErrorKind::StateUnknown,
        }
    }

    pub(crate) fn is_corrupt(&self) -> bool {
        matches!(self.kind, StoreErrorKind::Corrupt)
    }

    pub(crate) fn is_already_exists(&self) -> bool {
        matches!(self.kind, StoreErrorKind::AlreadyExists)
    }

    pub(crate) fn is_missing(&self) -> bool {
        matches!(self.kind, StoreErrorKind::Missing)
    }

    pub(crate) fn is_state_unknown(&self) -> bool {
        matches!(self.kind, StoreErrorKind::StateUnknown)
    }
}

pub(crate) trait CredentialStore: Send + Sync {
    fn load(&self, key: &CredentialKey) -> Result<Option<TokenBundle>, StoreError>;
    fn load_for_base_url(&self, base_url: &str) -> Result<Option<TokenBundle>, StoreError>;
    /// Create a new login credential and its base-URL index entry.
    fn create(&self, bundle: &TokenBundle) -> Result<(), StoreError>;
    /// Replace tokens for an existing key without changing its base-URL index.
    fn replace(&self, bundle: &TokenBundle) -> Result<(), StoreError>;
    fn delete(&self, key: &CredentialKey) -> Result<(), StoreError>;
}

pub(crate) struct KeyringCredentialStore {
    index_path: PathBuf,
    index_lock_path: PathBuf,
}

impl KeyringCredentialStore {
    pub(crate) fn new() -> Result<Self, StoreError> {
        let project_dirs = ProjectDirs::from("io", "project-jelly", "notegate-cli")
            .ok_or_else(StoreError::unavailable)?;
        Ok(Self {
            index_path: project_dirs.config_dir().join("profiles.json"),
            index_lock_path: project_dirs.data_local_dir().join("profiles.lock"),
        })
    }

    fn entry(key: &CredentialKey) -> Result<keyring::Entry, StoreError> {
        keyring::Entry::new(KEYRING_SERVICE, &keyring_account(key))
            .map_err(|_error| StoreError::unavailable())
    }

    fn read_index(&self) -> Result<ProfileIndex, StoreError> {
        let lock = lock_file(&self.index_lock_path)?;
        let _guard = lock.read().map_err(|_error| StoreError::unavailable())?;
        read_index_file(&self.index_path)
    }

    fn update_index(&self, update: impl FnOnce(&mut ProfileIndex)) -> Result<(), StoreError> {
        let mut lock = lock_file(&self.index_lock_path)?;
        let _guard = lock.write().map_err(|_error| StoreError::unavailable())?;
        let mut index = read_index_file(&self.index_path)?;
        update(&mut index);
        write_index_file(&self.index_path, &index)
    }
}

impl CredentialStore for KeyringCredentialStore {
    fn load(&self, key: &CredentialKey) -> Result<Option<TokenBundle>, StoreError> {
        let entry = Self::entry(key)?;
        let encoded = match entry.get_password() {
            Ok(encoded) => encoded,
            Err(keyring::Error::NoEntry) => return Ok(None),
            Err(_error) => return Err(StoreError::unavailable()),
        };
        let bundle = serde_json::from_str::<TokenBundle>(&encoded)
            .map_err(|_error| StoreError::corrupt())?;
        bundle.validate()?;
        if bundle.key() != *key {
            return Err(StoreError::corrupt());
        }
        Ok(Some(bundle))
    }

    fn load_for_base_url(&self, base_url: &str) -> Result<Option<TokenBundle>, StoreError> {
        let index = self.read_index()?;
        let Some(key) = index.profiles.get(base_url) else {
            return Ok(None);
        };
        let bundle = self.load(key)?;
        if bundle
            .as_ref()
            .is_some_and(|bundle| bundle.base_url != base_url)
        {
            return Err(StoreError::corrupt());
        }
        Ok(bundle)
    }

    fn create(&self, bundle: &TokenBundle) -> Result<(), StoreError> {
        bundle.validate()?;
        let key = bundle.key();
        let mut lock = lock_file(&self.index_lock_path)?;
        let _guard = lock.write().map_err(|_error| StoreError::unavailable())?;
        let mut index = read_index_file(&self.index_path)?;
        let entry = Self::entry(&key)?;
        match entry.get_password() {
            Ok(_existing) => return Err(StoreError::already_exists()),
            Err(keyring::Error::NoEntry) => {}
            Err(_error) => return Err(StoreError::unavailable()),
        }
        // A prior compensated/externally deleted credential can leave a
        // non-secret profile reference behind. It is safe to prune only after
        // confirming that its keychain entry no longer exists.
        index.profiles.retain(|_, candidate| candidate != &key);
        if let Some(indexed_key) = index.profiles.get(&bundle.base_url).cloned() {
            match Self::entry(&indexed_key)?.get_password() {
                Ok(_existing) => return Err(StoreError::already_exists()),
                Err(keyring::Error::NoEntry) => {
                    index.profiles.remove(&bundle.base_url);
                }
                Err(_error) => return Err(StoreError::unavailable()),
            }
        }
        let encoded = serde_json::to_string(bundle).map_err(|_error| StoreError::corrupt())?;
        index.profiles.insert(bundle.base_url.clone(), key);
        commit_new_credential(
            || {
                entry
                    .set_password(&encoded)
                    .map_err(|_error| StoreError::unavailable())
            },
            || write_index_file(&self.index_path, &index),
            || match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(_error) => Err(StoreError::unavailable()),
            },
        )
    }

    fn replace(&self, bundle: &TokenBundle) -> Result<(), StoreError> {
        bundle.validate()?;
        let key = bundle.key();
        let Some(existing) = self.load(&key)? else {
            return Err(StoreError::missing());
        };
        if existing.base_url != bundle.base_url {
            return Err(StoreError::corrupt());
        }
        let encoded = serde_json::to_string(bundle).map_err(|_error| StoreError::corrupt())?;
        Self::entry(&key)?
            .set_password(&encoded)
            .map_err(|_error| StoreError::unavailable())
    }

    fn delete(&self, key: &CredentialKey) -> Result<(), StoreError> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(_error) => return Err(StoreError::unavailable()),
        }
        let key = key.clone();
        self.update_index(move |index| {
            index.profiles.retain(|_, value| value != &key);
        })
    }
}

fn commit_new_credential(
    write_secret: impl FnOnce() -> Result<(), StoreError>,
    write_index: impl FnOnce() -> Result<(), StoreError>,
    cleanup: impl FnOnce() -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    write_secret()?;
    match write_index() {
        Ok(()) => Ok(()),
        Err(index_error) => match cleanup() {
            Ok(()) => Err(index_error),
            Err(_cleanup_error) => Err(StoreError::state_unknown()),
        },
    }
}

#[derive(Deserialize, Serialize)]
struct ProfileIndex {
    version: u8,
    profiles: BTreeMap<String, CredentialKey>,
}

impl Default for ProfileIndex {
    fn default() -> Self {
        Self {
            version: INDEX_VERSION,
            profiles: BTreeMap::new(),
        }
    }
}

fn read_index_file(path: &Path) -> Result<ProfileIndex, StoreError> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ProfileIndex::default());
        }
        Err(_error) => return Err(StoreError::unavailable()),
    };
    let mut encoded = String::new();
    file.read_to_string(&mut encoded)
        .map_err(|_error| StoreError::unavailable())?;
    let index =
        serde_json::from_str::<ProfileIndex>(&encoded).map_err(|_error| StoreError::corrupt())?;
    if index.version != INDEX_VERSION {
        return Err(StoreError::corrupt());
    }
    Ok(index)
}

fn write_index_file(path: &Path, index: &ProfileIndex) -> Result<(), StoreError> {
    let parent = path.parent().ok_or_else(StoreError::unavailable)?;
    fs::create_dir_all(parent).map_err(|_error| StoreError::unavailable())?;
    let encoded = serde_json::to_vec(index).map_err(|_error| StoreError::corrupt())?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = private_file(&temporary)?;
    file.set_len(0)
        .map_err(|_error| StoreError::unavailable())?;
    file.write_all(&encoded)
        .and_then(|()| file.sync_all())
        .map_err(|_error| StoreError::unavailable())?;
    fs::rename(&temporary, path).map_err(|_error| StoreError::unavailable())
}

pub(crate) fn lock_file(path: &Path) -> Result<RwLock<File>, StoreError> {
    let parent = path.parent().ok_or_else(StoreError::unavailable)?;
    fs::create_dir_all(parent).map_err(|_error| StoreError::unavailable())?;
    private_file(path).map(RwLock::new)
}

fn private_file(path: &Path) -> Result<File, StoreError> {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|_error| StoreError::unavailable())
}

pub(crate) fn key_digest(key: &CredentialKey) -> String {
    let mut digest = Sha256::new();
    digest.update(key.issuer.as_bytes());
    digest.update([0]);
    digest.update(key.client_id.as_bytes());
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn keyring_account(key: &CredentialKey) -> String {
    format!("oauth-device:{}", key_digest(key))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[test]
    fn failed_index_write_compensates_the_new_keyring_entry() {
        let wrote_secret = AtomicBool::new(false);
        let wrote_index = AtomicBool::new(false);
        let cleaned_up = AtomicBool::new(false);
        let result = commit_new_credential(
            || {
                wrote_secret.store(true, Ordering::SeqCst);
                Ok(())
            },
            || {
                wrote_index.store(true, Ordering::SeqCst);
                Err(StoreError::unavailable())
            },
            || {
                cleaned_up.store(true, Ordering::SeqCst);
                Ok(())
            },
        );
        assert!(result.is_err());
        let Err(error) = result else {
            return;
        };

        assert!(wrote_secret.load(Ordering::SeqCst));
        assert!(wrote_index.load(Ordering::SeqCst));
        assert!(cleaned_up.load(Ordering::SeqCst));
        assert!(!error.is_state_unknown());

        let result = commit_new_credential(
            || Ok(()),
            || Err(StoreError::unavailable()),
            || Err(StoreError::unavailable()),
        );
        assert!(result.is_err());
        let Err(unknown) = result else {
            return;
        };
        assert!(unknown.is_state_unknown());
    }

    #[test]
    fn token_bundle_debug_redacts_every_secret() {
        let bundle = TokenBundle::new(
            "http://localhost:9191".to_owned(),
            "https://auth.example.test".to_owned(),
            "notegate-cli-local".to_owned(),
            "https://auth.example.test/oauth/token".to_owned(),
            "https://auth.example.test/oauth/revoke".to_owned(),
            "access-secret-that-must-not-leak".to_owned(),
            "refresh-secret-that-must-not-leak".to_owned(),
            123,
        );

        let debug = format!("{bundle:?}");
        assert!(!debug.contains("access-secret"));
        assert!(!debug.contains("refresh-secret"));
        assert_eq!(debug.matches("[REDACTED]").count(), 2);
    }

    #[test]
    fn bundle_is_versioned_and_round_trips_only_inside_the_store() -> Result<(), StoreError> {
        let bundle = TokenBundle::new(
            "https://notegate.example".to_owned(),
            "https://auth.example.test".to_owned(),
            "notegate-cli".to_owned(),
            "https://auth.example.test/oauth/token".to_owned(),
            "https://auth.example.test/oauth/revoke".to_owned(),
            "access".to_owned(),
            "refresh".to_owned(),
            123,
        );

        let encoded = serde_json::to_string(&bundle).map_err(|_error| StoreError::corrupt())?;
        let decoded = serde_json::from_str::<TokenBundle>(&encoded)
            .map_err(|_error| StoreError::corrupt())?;
        decoded.validate()?;
        assert_eq!(decoded.version, BUNDLE_VERSION);
        assert_eq!(decoded.access_token.expose(), "access");
        assert_eq!(decoded.refresh_token.expose(), "refresh");
        Ok(())
    }

    #[test]
    fn key_digest_separates_local_and_production_clients() {
        let local = CredentialKey {
            issuer: "https://auth.example.test".to_owned(),
            client_id: "notegate-cli-local".to_owned(),
        };
        let production = CredentialKey {
            issuer: local.issuer.clone(),
            client_id: "notegate-cli".to_owned(),
        };

        assert_ne!(key_digest(&local), key_digest(&production));
    }
}
