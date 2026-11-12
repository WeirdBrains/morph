//! Morph identity helpers over Weirdnet 0.1.0.
//!
//! A person has one user key across Weirdnet apps. This crate uses
//! [`weirdnet_keys::Profile::WEIRDNET`] and [`weirdnet_net::user::user_id`] so a later
//! milestone can link Morph to an existing Muninn identity. First-run keyring storage,
//! SQLCipher unlock and device linking are milestone M3.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use weirdnet_keys::words::{from_words, to_words};
use weirdnet_keys::{Key32, KeysError, Profile};
use weirdnet_net::identity::{self, IdentityStore, Stored};
use weirdnet_net::user;
use weirdnet_net::{Keypair, NetError, PublicKey};
use zeroize::Zeroizing;

/// Errors from path resolution or Weirdnet identity types.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Keys(#[from] KeysError),
    #[error(transparent)]
    Net(#[from] NetError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Message(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Where Morph keeps its data: `home`, else `MORPH_HOME`, else `~/.morph`.
///
/// Resolving a path never creates the directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub home: PathBuf,
}

impl Paths {
    pub fn resolve(home: Option<PathBuf>) -> Result<Self> {
        let home = home.filter(|p| !p.as_os_str().is_empty()).or_else(|| std::env::var_os("MORPH_HOME").filter(|s| !s.is_empty()).map(PathBuf::from));
        let home = match home {
            Some(p) => p,
            None => dirs::home_dir().ok_or_else(|| Error::Message("no home directory".into()))?.join(".morph"),
        };
        Ok(Self { home })
    }

    pub fn exists(&self) -> bool {
        self.home.exists()
    }
}

/// Morph uses the shared Weirdnet key profile so identities can later be linked with Muninn.
pub fn profile() -> Profile {
    Profile::WEIRDNET
}

/// A newly generated user identity. The keypair stays in this process; only the public id
/// should cross a UI boundary.
pub struct UserIdentity {
    pub keypair: Keypair,
    pub user_id: String,
}

impl UserIdentity {
    /// A new Ed25519 user key and its Weirdnet user id.
    pub fn generate() -> Result<Self> {
        let keypair = Keypair::generate_ed25519();
        let user_id = user::user_id(&keypair.public());
        Ok(Self { keypair, user_id })
    }

    pub fn public_key(&self) -> PublicKey {
        self.keypair.public()
    }

    pub fn fingerprint(&self) -> String {
        identity::fingerprint(&self.public_key())
    }
}

/// A master key and its 24 recovery words (the keyring secret, not the user key).
pub struct MasterBackup {
    pub master: Key32,
    pub words: Zeroizing<String>,
}

impl MasterBackup {
    pub fn generate() -> Result<Self> {
        let master = Key32::random()?;
        let words = to_words(&master)?;
        Ok(Self { master, words })
    }

    pub fn from_words(words: &str) -> Result<Self> {
        let master = from_words(words)?;
        let words = to_words(&master)?;
        Ok(Self { master, words })
    }

    pub fn word_count(&self) -> usize {
        self.words.split_whitespace().count()
    }
}

/// In-memory node identity store, for tests and this first slice (no SQLCipher yet).
#[derive(Default)]
pub struct MemoryIdentityStore {
    inner: Mutex<Option<Stored>>,
}

impl IdentityStore for MemoryIdentityStore {
    type Error = Error;

    fn node_identity(&self) -> Result<Option<Stored>> {
        Ok(self.inner.lock().map_err(|_| Error::Message("identity store poisoned".into()))?.clone())
    }

    fn init_node_identity(&self, keypair: &[u8], peer: &str) -> Result<()> {
        let mut guard = self.inner.lock().map_err(|_| Error::Message("identity store poisoned".into()))?;
        if guard.is_none() {
            *guard = Some((Zeroizing::new(keypair.to_vec()), peer.to_string()));
        }
        Ok(())
    }
}

/// Load the node's keypair, creating one on first use.
pub fn load_or_create_node(store: &MemoryIdentityStore) -> Result<Keypair> {
    identity::load_or_create(store)
}

/// Public facts about a resolved Morph home. Never creates the directory.
pub fn identity_status(home: Option<&Path>) -> Result<IdentityStatus> {
    let paths = Paths::resolve(home.map(Path::to_path_buf))?;
    Ok(IdentityStatus {
        home: paths.home.display().to_string(),
        home_exists: paths.exists(),
        profile: profile().name.unwrap_or("weirdnet/keys/v1").to_string(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityStatus {
    pub home: String,
    pub home_exists: bool,
    pub profile: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use weirdnet_net::user::user_id;

    #[test]
    fn resolve_does_not_create_and_honours_override() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let status = identity_status(Some(&missing)).unwrap();
        assert!(!status.home_exists);
        assert!(!missing.exists());
        assert_eq!(status.profile, "weirdnet/keys/v1");
        assert_eq!(profile().name, Profile::WEIRDNET.name);
    }

    #[test]
    fn user_id_is_stable_and_public() {
        let user = UserIdentity::generate().unwrap();
        assert_eq!(user.user_id.len(), 32);
        assert!(user.user_id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(user.user_id, user_id(&user.public_key()));
        assert!(!user.fingerprint().is_empty());
        let again = UserIdentity::generate().unwrap();
        assert_ne!(user.user_id, again.user_id);
    }

    #[test]
    fn recovery_words_round_trip() {
        let backup = MasterBackup::generate().unwrap();
        assert_eq!(backup.word_count(), 24);
        let restored = MasterBackup::from_words(&backup.words).unwrap();
        assert_eq!(restored.master, backup.master);
    }

    #[test]
    fn node_key_is_stable_on_a_store() {
        let store = MemoryIdentityStore::default();
        let a = load_or_create_node(&store).unwrap();
        let b = load_or_create_node(&store).unwrap();
        assert_eq!(a.public().to_peer_id(), b.public().to_peer_id());
    }
}
