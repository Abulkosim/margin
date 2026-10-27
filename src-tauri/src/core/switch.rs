//! "Make active" swaps the sign-in of the CLI's default folder with another
//! folder, so each refresh-token family stays in exactly one place.
//! This is the only module that writes CLI credential or config files.

mod keychain;
mod lock;
mod process;
mod splice;

pub use lock::{LockError, LockSet, ProperLockfile};
pub use process::{running, Running};

use super::accounts;
use super::model::{Account, AccountSource, Provider};
use super::providers::claude::{keychain_service, keychain_suffix};
use base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};

const OAUTH_ACCOUNT: &str = "oauthAccount";
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const BACKUP_SERVICE: &str = "Margin backup";
const KEEP_RECORDS: usize = 5;
const KEEP_DAYS: i64 = 7;
const BACKUP_CHUNK: usize = 1_800;
const MAX_BACKUP_CHUNKS: usize = 64;
const RESTORE_MINUTES: i64 = 30;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Location {
    Keychain { service: String, account: String },
    File { path: PathBuf },
    Config { path: PathBuf },
}

impl Location {
    /// Credentials compare whole values; a config compares only `oauthAccount`,
    /// because the CLI rewrites the rest of that file all the time.
    fn fingerprint(&self, bytes: Option<&[u8]>) -> Option<String> {
        let bytes = bytes?;
        match self {
            Self::Config { .. } => Some(
                std::str::from_utf8(bytes)
                    .ok()
                    .and_then(|text| splice::raw_value(text, OAUTH_ACCOUNT).ok())
                    .map(
                        |raw| match raw.and_then(|raw| serde_json::from_str::<Value>(raw).ok()) {
                            Some(value) => sha(value.to_string().as_bytes()),
                            None => "absent".into(),
                        },
                    )
                    .unwrap_or_else(|| format!("unreadable-{}", sha(bytes))),
            ),
            _ => Some(match serde_json::from_slice::<Value>(bytes) {
                Ok(value) => sha(value.to_string().as_bytes()),
                Err(_) => sha(bytes),
            }),
        }
    }

    /// Keychain values go in as compact JSON: `security -w` prints a secret that
    /// contains a newline as hex, which would never read back equal.
    fn stored(&self, bytes: &[u8]) -> Vec<u8> {
        match self {
            Self::Keychain { .. } => serde_json::from_slice::<Value>(bytes)
                .map(|value| value.to_string().into_bytes())
                .unwrap_or_else(|_| bytes.to_vec()),
            _ => bytes.to_vec(),
        }
    }

    fn path(&self) -> Option<&Path> {
        match self {
            Self::File { path } | Self::Config { path } => Some(path),
            Self::Keychain { .. } => None,
        }
    }
}

fn sha(bytes: &[u8]) -> String {
    super::hex(&Sha256::digest(bytes))
}

#[derive(Debug, PartialEq, Eq)]
pub enum StoreError {
    Keychain,
    Io,
}

pub trait CredentialStore {
    fn read(&self, location: &Location) -> Result<Option<Vec<u8>>, StoreError>;
    fn write(&self, location: &Location, bytes: &[u8]) -> Result<(), StoreError>;
    fn clear(&self, location: &Location) -> Result<(), StoreError>;
    fn keychain_account(&self, service: &str) -> Result<Option<String>, StoreError>;
    fn is_symlink(&self, path: &Path) -> bool;
}

/// Keychain items through `/usr/bin/security`, files through temp + fsync + rename.
pub struct SystemStore;

impl CredentialStore for SystemStore {
    fn read(&self, location: &Location) -> Result<Option<Vec<u8>>, StoreError> {
        match location {
            Location::Keychain { service, .. } => read_keychain(service),
            Location::File { path } | Location::Config { path } => read_file(path),
        }
    }

    fn write(&self, location: &Location, bytes: &[u8]) -> Result<(), StoreError> {
        match location {
            Location::Keychain { service, account } => write_keychain(service, account, bytes),
            Location::File { path } | Location::Config { path } => write_file(path, bytes),
        }
    }

    fn clear(&self, location: &Location) -> Result<(), StoreError> {
        match location {
            Location::Keychain { service, account } => remove_keychain(service, account),
            Location::File { path } => match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(StoreError::Io),
            },
            Location::Config { .. } => Err(StoreError::Io),
        }
    }

    fn keychain_account(&self, service: &str) -> Result<Option<String>, StoreError> {
        #[cfg(target_os = "macos")]
        {
            keychain::account(service).map_err(|_| StoreError::Keychain)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = service;
            Err(StoreError::Keychain)
        }
    }

    fn is_symlink(&self, path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
    }
}

fn read_keychain(service: &str) -> Result<Option<Vec<u8>>, StoreError> {
    #[cfg(target_os = "macos")]
    {
        keychain::read(service).map_err(|_| StoreError::Keychain)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = service;
        Err(StoreError::Keychain)
    }
}

fn write_keychain(service: &str, account: &str, bytes: &[u8]) -> Result<(), StoreError> {
    #[cfg(target_os = "macos")]
    {
        keychain::write(service, account, bytes).map_err(|_| StoreError::Keychain)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (service, account, bytes);
        Err(StoreError::Keychain)
    }
}

fn remove_keychain(service: &str, account: &str) -> Result<(), StoreError> {
    #[cfg(target_os = "macos")]
    {
        keychain::remove(service, account).map_err(|_| StoreError::Keychain)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (service, account);
        Err(StoreError::Keychain)
    }
}

fn read_file(path: &Path) -> Result<Option<Vec<u8>>, StoreError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(StoreError::Io),
    };
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err(StoreError::Io);
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| StoreError::Io)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| StoreError::Io)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(StoreError::Io);
    }
    Ok(Some(bytes))
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let parent = path.parent().ok_or(StoreError::Io)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| StoreError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|_| StoreError::Io)?;
    }
    file.write_all(bytes).map_err(|_| StoreError::Io)?;
    file.as_file().sync_all().map_err(|_| StoreError::Io)?;
    file.persist(path).map_err(|_| StoreError::Io)?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotSpec {
    pub provider: Provider,
    pub dir: PathBuf,
    pub default: bool,
    pub credentials: Vec<Location>,
    pub config: Option<PathBuf>,
    pub codex_config: Option<PathBuf>,
    pub locks: Vec<PathBuf>,
}

pub struct Context<'a> {
    pub home: &'a Path,
    pub keychain: bool,
    pub store: &'a dyn CredentialStore,
}

pub fn default_dir(provider: Provider, home: &Path) -> PathBuf {
    home.join(format!(".{}", provider.key()))
}

/// The folder the CLI uses without `CLAUDE_CONFIG_DIR` / `CODEX_HOME`.
pub fn is_default_source(provider: Provider, source: &AccountSource, home: &Path) -> bool {
    match source {
        AccountSource::Keychain => provider == Provider::Claude,
        AccountSource::ConfigDir { path, .. } => {
            same_dir(Path::new(path), &default_dir(provider, home))
        }
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a == b
        || std::fs::canonicalize(a)
            .ok()
            .zip(std::fs::canonicalize(b).ok())
            .is_some_and(|(a, b)| a == b)
}

pub fn active_ids(accounts: &[Account]) -> Vec<String> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    accounts
        .iter()
        .filter(|account| is_default_source(account.provider, &account.source, &home))
        .map(|account| account.id.clone())
        .collect()
}

pub fn default_source(provider: Provider, home: &Path) -> AccountSource {
    let dir = default_dir(provider, home);
    let path = std::fs::canonicalize(&dir).unwrap_or(dir);
    AccountSource::ConfigDir {
        path: path.to_string_lossy().into_owned(),
        keychain_suffix: None,
    }
}

pub fn slot_spec(
    provider: Provider,
    source: &AccountSource,
    context: &Context,
) -> Result<SlotSpec, SwitchError> {
    let default = is_default_source(provider, source, context.home);
    let dir = match source {
        AccountSource::ConfigDir { path, .. } => PathBuf::from(path),
        AccountSource::Keychain => default_dir(Provider::Claude, context.home),
    };
    match provider {
        Provider::Claude => {
            let mut credentials = Vec::new();
            if context.keychain {
                let service = match source {
                    _ if default => keychain_service(None),
                    AccountSource::ConfigDir {
                        keychain_suffix: Some(suffix),
                        ..
                    } => keychain_service(Some(suffix)),
                    AccountSource::ConfigDir { path, .. } => {
                        keychain_service(Some(&keychain_suffix(path)))
                    }
                    AccountSource::Keychain => keychain_service(None),
                };
                let account = context
                    .store
                    .keychain_account(&service)?
                    .unwrap_or_else(keychain::default_account);
                credentials.push(Location::Keychain { service, account });
            }
            credentials.push(Location::File {
                path: dir.join(".credentials.json"),
            });
            let config = global_config(&dir, default, context)?;
            let real = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
            let locks = vec![
                dir.join(".oauth_refresh.lock"),
                with_suffix(&real, ".lock"),
                dir.join(".storage-write.lock"),
                with_suffix(&config, ".lock"),
            ];
            Ok(SlotSpec {
                provider,
                dir,
                default,
                credentials,
                config: Some(config),
                codex_config: None,
                locks,
            })
        }
        Provider::Codex => Ok(SlotSpec {
            provider,
            credentials: vec![Location::File {
                path: dir.join("auth.json"),
            }],
            config: None,
            codex_config: Some(dir.join("config.toml")),
            locks: Vec::new(),
            dir,
            default,
        }),
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(suffix);
    PathBuf::from(value)
}

/// Claude Code's own rule: a non-empty legacy `.config.json` wins, else
/// `~/.claude.json` for the default folder and `<dir>/.claude.json` for others.
fn global_config(dir: &Path, default: bool, context: &Context) -> Result<PathBuf, SwitchError> {
    let legacy = dir.join(".config.json");
    if context
        .store
        .read(&Location::Config {
            path: legacy.clone(),
        })?
        .is_some_and(|bytes| !bytes.is_empty())
    {
        return Ok(legacy);
    }
    Ok(if default {
        context.home.join(".claude.json")
    } else {
        dir.join(".claude.json")
    })
}

#[derive(Clone, Debug)]
pub struct SlotState {
    pub spec: SlotSpec,
    /// Every credential location that holds a value, in Claude Code's order.
    pub values: Vec<(Location, Vec<u8>)>,
    /// The first location holding a login.
    pub credential: Option<(Location, Vec<u8>)>,
    pub config: Option<Vec<u8>>,
    pub symlink: Option<PathBuf>,
    pub keyring: Option<bool>,
    pub unreadable: bool,
    pub conflict: bool,
}

pub fn read_slot(spec: &SlotSpec, store: &dyn CredentialStore) -> Result<SlotState, SwitchError> {
    let mut state = SlotState {
        spec: spec.clone(),
        values: Vec::new(),
        credential: None,
        config: None,
        symlink: None,
        keyring: Some(false),
        unreadable: false,
        conflict: false,
    };
    let paths = spec
        .credentials
        .iter()
        .filter_map(Location::path)
        .chain(spec.config.as_deref())
        .chain(spec.codex_config.as_deref());
    if let Some(path) = paths.into_iter().find(|path| store.is_symlink(path)) {
        state.symlink = Some(path.to_path_buf());
        return Ok(state);
    }
    for location in &spec.credentials {
        if let Some(bytes) = store.read(location)? {
            state.values.push((location.clone(), bytes));
        }
    }
    let mut logins = Vec::new();
    for (location, bytes) in &state.values {
        let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
            state.unreadable = true;
            continue;
        };
        if let Some(family) = login_family(spec.provider, &value) {
            logins.push((location.clone(), bytes.clone(), family));
        }
    }
    state.conflict = logins
        .iter()
        .any(|(_, _, family)| Some(family) != logins.first().map(|(_, _, first)| first));
    state.credential = logins
        .into_iter()
        .next()
        .map(|(location, bytes, _)| (location, bytes));
    if let Some(path) = &spec.config {
        state.config = store.read(&Location::Config { path: path.clone() })?;
    }
    if let Some(path) = &spec.codex_config {
        state.keyring = match store.read(&Location::File { path: path.clone() })? {
            None => Some(false),
            Some(bytes) => std::str::from_utf8(&bytes)
                .ok()
                .and_then(|text| text.parse::<toml::Table>().ok())
                .map(|table| {
                    table
                        .get("cli_auth_credentials_store")
                        .and_then(toml::Value::as_str)
                        .is_some_and(|mode| matches!(mode, "keyring" | "auto"))
                }),
        };
    }
    Ok(state)
}

/// A location counts as a login only with an access token; the refresh token
/// (else the access token) names the token family.
fn login_family(provider: Provider, value: &Value) -> Option<String> {
    let (tokens, access, refresh) = match provider {
        Provider::Claude => (value.get("claudeAiOauth")?, "accessToken", "refreshToken"),
        Provider::Codex => (value.get("tokens")?, "access_token", "refresh_token"),
    };
    let access = text_field(tokens.get(access))?;
    Some(text_field(tokens.get(refresh)).unwrap_or(access))
}

/// Codex has no lock: an in-flight refresh could write the displaced tokens
/// back after the switch, so a running Codex is refused, not warned about.
pub fn check_running(provider: Provider, running: Running) -> Result<(), Refusal> {
    if provider == Provider::Codex && running.any() {
        return Err(Refusal::CodexRunning {
            helper: running == Running::Helper,
        });
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    CodexRunning { helper: bool },
    TwoSignIns,
    RestoreStale,
    RestoreExpired,
    NotOffered,
    SameAccount,
    SourceSignedOut,
    DefaultSignedOut,
    Unreadable(PathBuf),
    ApiKey,
    CodexKeyring,
    CodexConfig(PathBuf),
    Symlink(PathBuf),
    ConfigMissing(PathBuf),
    ConfigUnreadable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanKind {
    Swap,
    Move,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub provider: Provider,
    pub kind: PlanKind,
    pub fingerprint: String,
    pub target_email: Option<String>,
    pub displaced_email: Option<String>,
}

struct Login {
    key: Option<String>,
    email: Option<String>,
    refresh: bool,
}

/// Pure: decides whether and how the source can become the default sign-in.
pub fn plan(default: &SlotState, source: &SlotState) -> Result<Plan, Refusal> {
    let provider = source.spec.provider;
    if !default.spec.default || source.spec.default || default.spec.provider != provider {
        return Err(Refusal::NotOffered);
    }
    for state in [default, source] {
        if let Some(path) = &state.symlink {
            return Err(Refusal::Symlink(path.clone()));
        }
        match state.keyring {
            Some(true) => return Err(Refusal::CodexKeyring),
            None => return Err(Refusal::CodexConfig(state.spec.dir.clone())),
            Some(false) => {}
        }
        if state.unreadable {
            return Err(Refusal::Unreadable(state.spec.dir.clone()));
        }
        if state.conflict {
            return Err(Refusal::TwoSignIns);
        }
    }
    let source_login = login(source)?.ok_or(Refusal::SourceSignedOut)?;
    let Some(source_key) = source_login.key.filter(|_| source_login.refresh) else {
        return Err(Refusal::SourceSignedOut);
    };
    let default_login = login(default)?;
    let default_key = match &default_login {
        None => None,
        Some(Login {
            key: Some(key),
            refresh: true,
            ..
        }) => Some(key.clone()),
        Some(_) => return Err(Refusal::DefaultSignedOut),
    };
    if default_key.as_ref() == Some(&source_key) {
        return Err(Refusal::SameAccount);
    }
    let kind = if default_key.is_some() {
        PlanKind::Swap
    } else {
        PlanKind::Move
    };
    let mut digest = Sha256::new();
    for part in [
        provider.key(),
        if kind == PlanKind::Swap {
            "swap"
        } else {
            "move"
        },
        &default.spec.dir.to_string_lossy(),
        &source.spec.dir.to_string_lossy(),
        default_key.as_deref().unwrap_or(""),
        &source_key,
    ] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    Ok(Plan {
        provider,
        kind,
        fingerprint: super::hex(&digest.finalize()),
        target_email: source_login.email,
        displaced_email: default_login.and_then(|login| login.email),
    })
}

fn login(state: &SlotState) -> Result<Option<Login>, Refusal> {
    match state.spec.provider {
        Provider::Claude => claude_login(state),
        Provider::Codex => codex_login(state),
    }
}

fn claude_login(state: &SlotState) -> Result<Option<Login>, Refusal> {
    let path = state.spec.config.clone().unwrap_or_default();
    let text = state
        .config
        .as_deref()
        .ok_or(Refusal::ConfigMissing(path))?;
    let text = std::str::from_utf8(text).map_err(|_| Refusal::ConfigUnreadable)?;
    let account = splice::raw_value(text, OAUTH_ACCOUNT)
        .map_err(|_| Refusal::ConfigUnreadable)?
        .map(serde_json::from_str::<Value>)
        .transpose()
        .map_err(|_| Refusal::ConfigUnreadable)?;
    let Some((_, bytes)) = &state.credential else {
        return Ok(None);
    };
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| Refusal::Unreadable(state.spec.dir.clone()))?;
    let Some(oauth) = value.get("claudeAiOauth").filter(|oauth| oauth.is_object()) else {
        return Ok(None);
    };
    if text_field(oauth.get("accessToken")).is_none() {
        return Ok(None);
    }
    let field = |name| {
        account
            .as_ref()
            .and_then(|account| text_field(account.get(name)))
    };
    Ok(Some(Login {
        key: field("accountUuid")
            .zip(field("organizationUuid"))
            .map(|(account, organization)| format!("{account}/{organization}")),
        email: field("emailAddress"),
        refresh: text_field(oauth.get("refreshToken")).is_some(),
    }))
}

fn codex_login(state: &SlotState) -> Result<Option<Login>, Refusal> {
    if state.values.iter().any(|(_, bytes)| {
        serde_json::from_slice::<Value>(bytes)
            .is_ok_and(|value| value.get("auth_mode").and_then(Value::as_str) == Some("apikey"))
    }) {
        return Err(Refusal::ApiKey);
    }
    let Some((_, bytes)) = &state.credential else {
        return Ok(None);
    };
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| Refusal::Unreadable(state.spec.dir.clone()))?;
    let Some(tokens) = value.get("tokens").filter(|tokens| tokens.is_object()) else {
        return Ok(None);
    };
    let Some(access) = text_field(tokens.get("access_token")) else {
        return Ok(None);
    };
    let claims: Vec<Value> = [Some(access), text_field(tokens.get("id_token"))]
        .into_iter()
        .flatten()
        .filter_map(|token| jwt_claims(&token))
        .collect();
    let claim = |pointer: &str| {
        claims
            .iter()
            .find_map(|claims| text_field(claims.pointer(pointer)))
    };
    let account = text_field(tokens.get("account_id"))
        .or_else(|| claim("/https:~1~1api.openai.com~1auth/chatgpt_account_id"));
    let user = claim("/https:~1~1api.openai.com~1auth/chatgpt_user_id").or_else(|| claim("/sub"));
    Ok(Some(Login {
        key: account
            .zip(user)
            .map(|(account, user)| format!("{account}/{user}")),
        email: claim("/email").or_else(|| claim("/https:~1~1api.openai.com~1profile/email")),
        refresh: text_field(tokens.get("refresh_token")).is_some(),
    }))
}

fn text_field(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 65_536)
        .map(str::to_owned)
}

fn jwt_claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| URL_SAFE.decode(payload))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Write {
    pub location: Location,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}

/// Credentials first (default slot, then source), then `oauthAccount` in both
/// Claude config files. Every other config key is kept byte for byte.
pub fn writes(plan: &Plan, default: &SlotState, source: &SlotState) -> Result<Vec<Write>, Refusal> {
    let (_, source_bytes) = source.credential.clone().ok_or(Refusal::SourceSignedOut)?;
    // The CLI reads the first location holding any value, so the incoming
    // sign-in goes there; every other copy in either slot is cleared.
    let target = |state: &SlotState| {
        state
            .values
            .first()
            .map(|(location, _)| location.clone())
            .or_else(|| state.spec.credentials.first().cloned())
            .ok_or(Refusal::NotOffered)
    };
    let current = |state: &SlotState, location: &Location| {
        state
            .values
            .iter()
            .find(|(known, _)| known == location)
            .map(|(_, bytes)| bytes.clone())
    };
    let mut writes = Vec::new();
    let mut written = Vec::new();
    let default_location = target(default)?;
    writes.push(Write {
        before: current(default, &default_location),
        after: Some(default_location.stored(&source_bytes)),
        location: default_location.clone(),
    });
    written.push(default_location);
    if plan.kind == PlanKind::Swap {
        let (_, default_bytes) = default.credential.clone().ok_or(Refusal::NotOffered)?;
        let source_location = target(source)?;
        writes.push(Write {
            before: current(source, &source_location),
            after: Some(source_location.stored(&default_bytes)),
            location: source_location.clone(),
        });
        written.push(source_location);
    }
    for (location, bytes) in default.values.iter().chain(&source.values) {
        if !written.contains(location) {
            writes.push(Write {
                location: location.clone(),
                before: Some(bytes.clone()),
                after: None,
            });
        }
    }
    if plan.provider == Provider::Claude {
        let text = |state: &SlotState| -> Result<(PathBuf, String), Refusal> {
            let path = state.spec.config.clone().ok_or(Refusal::ConfigUnreadable)?;
            let bytes = state
                .config
                .clone()
                .ok_or_else(|| Refusal::ConfigMissing(path.clone()))?;
            Ok((
                path,
                String::from_utf8(bytes).map_err(|_| Refusal::ConfigUnreadable)?,
            ))
        };
        let (default_path, default_text) = text(default)?;
        let (source_path, source_text) = text(source)?;
        let unreadable = |_| Refusal::ConfigUnreadable;
        let source_account = splice::raw_value(&source_text, OAUTH_ACCOUNT).map_err(unreadable)?;
        let default_account =
            splice::raw_value(&default_text, OAUTH_ACCOUNT).map_err(unreadable)?;
        let next_default =
            splice::splice(&default_text, OAUTH_ACCOUNT, source_account).map_err(unreadable)?;
        let next_source = splice::splice(
            &source_text,
            OAUTH_ACCOUNT,
            match plan.kind {
                PlanKind::Swap => default_account,
                PlanKind::Move => None,
            },
        )
        .map_err(unreadable)?;
        writes.push(Write {
            location: Location::Config { path: default_path },
            before: Some(default_text.clone().into_bytes()),
            after: Some(next_default.into_bytes()),
        });
        writes.push(Write {
            location: Location::Config { path: source_path },
            before: Some(source_text.clone().into_bytes()),
            after: Some(next_source.into_bytes()),
        });
    }
    Ok(writes)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordState {
    Started,
    Written,
    Done,
    RolledBack,
    RollbackFailed,
    VerifyFailed,
    Abandoned,
    Restored,
    Dismissed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordItem {
    pub location: Location,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordAccounts {
    pub target: String,
    pub previous: Option<String>,
    pub target_source: AccountSource,
    pub default_source: AccountSource,
}

/// Journal entry. Holds locations and fingerprints only, never secrets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchRecord {
    pub id: String,
    pub provider: Provider,
    pub created_at: String,
    pub restore: bool,
    pub state: RecordState,
    pub items: Vec<RecordItem>,
    pub locks: Vec<PathBuf>,
    #[serde(default)]
    pub accounts: Option<RecordAccounts>,
    #[serde(default)]
    pub target_email: Option<String>,
    #[serde(default)]
    pub displaced_email: Option<String>,
    /// Fingerprints of the stores when the switch failed; a restore is safe
    /// only while they still match.
    #[serde(default)]
    pub live: Option<Vec<Option<String>>>,
}

impl SwitchRecord {
    fn age_minutes(&self, now: chrono::DateTime<chrono::Utc>) -> Option<i64> {
        chrono::DateTime::parse_from_rfc3339(&self.created_at)
            .ok()
            .map(|at| (now - at.with_timezone(&chrono::Utc)).num_minutes())
    }

    /// Failed or unfinished, with live fingerprints, and under 30 minutes old.
    pub fn restorable(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        matches!(
            self.state,
            RecordState::Started | RecordState::VerifyFailed | RecordState::RollbackFailed
        ) && self.live.is_some()
            && self
                .age_minutes(now)
                .is_some_and(|age| (0..RESTORE_MINUTES).contains(&age))
    }
}

fn fingerprints(store: &dyn CredentialStore, items: &[RecordItem]) -> Vec<Option<String>> {
    items
        .iter()
        .map(|item| match store.read(&item.location) {
            Ok(bytes) => item.location.fingerprint(bytes.as_deref()),
            Err(_) => Some("unreadable".into()),
        })
        .collect()
}

#[derive(Serialize, Deserialize)]
struct BackupBlob {
    version: u32,
    id: String,
    items: Vec<BackupItem>,
}

#[derive(Serialize, Deserialize)]
struct BackupItem {
    location: Location,
    value: Option<String>,
}

pub trait Journal {
    fn records(&self) -> Vec<SwitchRecord>;
    fn begin(&mut self, record: &SwitchRecord, backup: &[u8]) -> Result<(), SwitchError>;
    fn mark(&mut self, id: &str, state: RecordState) -> Result<(), SwitchError>;
    /// Marks a failure and keeps what the stores held at that moment.
    fn fail(
        &mut self,
        id: &str,
        state: RecordState,
        live: Vec<Option<String>>,
    ) -> Result<(), SwitchError>;
    fn backup(&self, id: &str) -> Result<Option<Vec<u8>>, SwitchError>;
}

#[derive(Clone, Debug, PartialEq)]
pub enum SwitchError {
    Refused(Refusal),
    Changed,
    Busy,
    Keychain,
    Io,
    Journal,
    RolledBack { keychain: bool },
    RollbackFailed { id: String },
    VerifyFailed { id: String },
}

impl From<Refusal> for SwitchError {
    fn from(refusal: Refusal) -> Self {
        Self::Refused(refusal)
    }
}

impl From<StoreError> for SwitchError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Keychain => Self::Keychain,
            StoreError::Io => Self::Io,
        }
    }
}

impl From<LockError> for SwitchError {
    fn from(error: LockError) -> Self {
        match error {
            LockError::Busy => Self::Busy,
            LockError::Unavailable => Self::Io,
        }
    }
}

/// What a person sees. `label` and `folder` name the source account.
pub fn message(error: &SwitchError, provider: Provider, label: &str, folder: &Path) -> String {
    let cli = match provider {
        Provider::Claude => "Claude Code",
        Provider::Codex => "Codex",
    };
    let home = dirs::home_dir().unwrap_or_default();
    match error {
        SwitchError::Refused(refusal) => match refusal {
            Refusal::NotOffered => "Only another folder of the same CLI can be made active.".into(),
            Refusal::CodexRunning { helper } => {
                let mut text = String::from("Codex is still running. Quit the Codex app and any Codex CLI or editor sessions, then try again.");
                if *helper {
                    text.push_str("\nA Codex background helper is still running. It stops a few minutes after the app quits.");
                }
                text
            }
            Refusal::TwoSignIns => format!(
                "This folder has two different sign-ins. Sign in again with {} first.",
                provider.key()
            ),
            Refusal::RestoreStale => {
                "The sign-ins changed since then. Restore is no longer safe.".into()
            }
            Refusal::RestoreExpired => {
                "This backup is more than 30 minutes old. Restore is no longer safe.".into()
            }
            Refusal::SameAccount => {
                "Both folders hold the same account. Sign out of one first.".into()
            }
            Refusal::SourceSignedOut => format!(
                "{label} is signed out. Run {}, then try again.",
                super::providers::sign_in_command(provider, Some(folder))
            ),
            Refusal::DefaultSignedOut => format!(
                "The active {} sign-in can’t be moved. Run {}, then try again.",
                provider.name(),
                super::providers::sign_in_command(provider, None)
            ),
            Refusal::Unreadable(dir) => format!(
                "{} sign-in in {} couldn’t be read.",
                provider.name(),
                display_path(dir, &home)
            ),
            Refusal::ApiKey => {
                "This Codex folder uses an API key. Only ChatGPT sign-ins can be switched.".into()
            }
            Refusal::CodexKeyring => {
                "Codex keeps this sign-in in the system keyring. Use `codex login` to switch."
                    .into()
            }
            Refusal::CodexConfig(dir) => format!(
                "Codex config.toml in {} couldn’t be read.",
                display_path(dir, &home)
            ),
            Refusal::Symlink(path) => format!(
                "{} is a symbolic link. Margin won’t replace it.",
                display_path(path, &home)
            ),
            Refusal::ConfigMissing(path) => format!(
                "{} wasn’t found. Open {cli} in that folder once, then try again.",
                display_path(path, &home)
            ),
            Refusal::ConfigUnreadable => {
                "Claude Code’s settings file couldn’t be read. Margin won’t change it.".into()
            }
        },
        SwitchError::Changed => "The accounts changed. Review and try again.".into(),
        SwitchError::Busy => format!("{cli} is busy. Try again in a moment."),
        SwitchError::Keychain | SwitchError::RolledBack { keychain: true } => {
            "Keychain access was denied. Unlock it and try again.".into()
        }
        SwitchError::Io | SwitchError::RolledBack { keychain: false } => {
            "Couldn’t switch accounts. Nothing was changed.".into()
        }
        SwitchError::Journal => "Margin couldn’t save a backup, so nothing was changed.".into(),
        SwitchError::RollbackFailed { .. } => {
            "Couldn’t switch accounts or undo it. Restore the backup in Settings → Accounts.".into()
        }
        SwitchError::VerifyFailed { .. } => format!("{cli} changed the sign-in during the switch."),
    }
}

pub fn display_path(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if !home.as_os_str().is_empty() => {
            format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display())
        }
        _ => path.display().to_string(),
    }
}

#[derive(Debug)]
pub struct Outcome {
    pub plan: Plan,
    pub record: SwitchRecord,
}

/// Locks, re-reads, re-plans, backs up, writes, verifies, rolls back on failure.
pub fn execute(
    expected: &str,
    default: &SlotSpec,
    source: &SlotSpec,
    accounts: Option<RecordAccounts>,
    store: &dyn CredentialStore,
    locks: &dyn LockSet,
    journal: &mut dyn Journal,
) -> Result<Outcome, SwitchError> {
    let paths = [default.locks.clone(), source.locks.clone()].concat();
    let _held = locks.acquire(&paths)?;
    let default_state = read_slot(default, store)?;
    let source_state = read_slot(source, store)?;
    let plan = plan(&default_state, &source_state)?;
    if plan.fingerprint != expected {
        return Err(SwitchError::Changed);
    }
    let writes = writes(&plan, &default_state, &source_state)?;
    let mut record = new_record(plan.provider, false, &writes, lock::lock_order(&paths));
    record.accounts = accounts;
    record.target_email = plan.target_email.clone();
    record.displaced_email = plan.displaced_email.clone();
    let record = commit(record, &writes, store, journal)?;
    Ok(Outcome { plan, record })
}

/// Writes a backup's values back, under the same locks, as a journaled operation.
pub fn restore(
    id: &str,
    now: chrono::DateTime<chrono::Utc>,
    store: &dyn CredentialStore,
    locks: &dyn LockSet,
    journal: &mut dyn Journal,
) -> Result<SwitchRecord, SwitchError> {
    let original = journal
        .records()
        .into_iter()
        .find(|record| record.id == id)
        .ok_or(SwitchError::Journal)?;
    if !original.restorable(now) {
        return Err(Refusal::RestoreExpired.into());
    }
    let blob: BackupBlob = journal
        .backup(id)?
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or(SwitchError::Journal)?;
    let _held = locks.acquire(&original.locks)?;
    if original.live.as_ref() != Some(&fingerprints(store, &original.items)) {
        return Err(Refusal::RestoreStale.into());
    }
    let mut writes = Vec::new();
    for item in blob.items {
        let current = store.read(&item.location)?;
        let after = match &item.location {
            Location::Config { .. } => {
                let text = current
                    .as_deref()
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .ok_or(Refusal::ConfigUnreadable)?;
                Some(
                    splice::splice(text, OAUTH_ACCOUNT, item.value.as_deref())
                        .map_err(|_| Refusal::ConfigUnreadable)?
                        .into_bytes(),
                )
            }
            location => item
                .value
                .map(|value| {
                    STANDARD
                        .decode(value)
                        .map(|bytes| location.stored(&bytes))
                        .map_err(|_| SwitchError::Journal)
                })
                .transpose()?,
        };
        if current != after {
            writes.push(Write {
                location: item.location,
                before: current,
                after,
            });
        }
    }
    let record = new_record(original.provider, true, &writes, original.locks.clone());
    let record = commit(record, &writes, store, journal)?;
    journal.mark(id, RecordState::Restored)?;
    Ok(record)
}

fn new_record(
    provider: Provider,
    restore: bool,
    writes: &[Write],
    locks: Vec<PathBuf>,
) -> SwitchRecord {
    let now = chrono::Utc::now();
    let nonce = sha(format!(
        "{}-{}-{:?}",
        now.timestamp_nanos_opt().unwrap_or_default(),
        std::process::id(),
        std::thread::current().id()
    )
    .as_bytes());
    SwitchRecord {
        id: format!(
            "{}-{}-{}",
            provider.key(),
            now.format("%Y%m%dT%H%M%SZ"),
            &nonce[..8]
        ),
        provider,
        created_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        restore,
        state: RecordState::Started,
        items: writes
            .iter()
            .map(|write| RecordItem {
                location: write.location.clone(),
                before: write.location.fingerprint(write.before.as_deref()),
                after: write.location.fingerprint(write.after.as_deref()),
            })
            .collect(),
        locks,
        accounts: None,
        target_email: None,
        displaced_email: None,
        live: None,
    }
}

fn backup_blob(id: &str, writes: &[Write]) -> Result<Vec<u8>, SwitchError> {
    let items = writes
        .iter()
        .map(|write| {
            let value = match &write.location {
                Location::Config { .. } => write
                    .before
                    .as_deref()
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .and_then(|text| splice::raw_value(text, OAUTH_ACCOUNT).ok().flatten())
                    .map(str::to_owned),
                _ => write.before.as_deref().map(|bytes| STANDARD.encode(bytes)),
            };
            BackupItem {
                location: write.location.clone(),
                value,
            }
        })
        .collect();
    serde_json::to_vec(&BackupBlob {
        version: 1,
        id: id.to_owned(),
        items,
    })
    .map_err(|_| SwitchError::Journal)
}

fn commit(
    mut record: SwitchRecord,
    writes: &[Write],
    store: &dyn CredentialStore,
    journal: &mut dyn Journal,
) -> Result<SwitchRecord, SwitchError> {
    journal.begin(&record, &backup_blob(&record.id, writes)?)?;
    for (index, write) in writes.iter().enumerate() {
        if let Err(error) = apply(store, write) {
            return if rollback(store, &writes[..=index]) {
                let _ = journal.mark(&record.id, RecordState::RolledBack);
                Err(SwitchError::RolledBack {
                    keychain: error == StoreError::Keychain,
                })
            } else {
                let live = fingerprints(store, &record.items);
                let _ = journal.fail(&record.id, RecordState::RollbackFailed, live);
                Err(SwitchError::RollbackFailed { id: record.id })
            };
        }
    }
    if !verify(store, &record) {
        let live = fingerprints(store, &record.items);
        let _ = journal.fail(&record.id, RecordState::VerifyFailed, live);
        return Err(SwitchError::VerifyFailed { id: record.id });
    }
    record.state = RecordState::Written;
    let _ = journal.mark(&record.id, RecordState::Written);
    Ok(record)
}

fn apply(store: &dyn CredentialStore, write: &Write) -> Result<(), StoreError> {
    match &write.after {
        Some(bytes) => store.write(&write.location, bytes)?,
        None => store.clear(&write.location)?,
    }
    let now = store.read(&write.location)?;
    if write.location.fingerprint(now.as_deref())
        != write.location.fingerprint(write.after.as_deref())
    {
        return Err(StoreError::Io);
    }
    Ok(())
}

fn rollback(store: &dyn CredentialStore, writes: &[Write]) -> bool {
    let mut restored = true;
    for write in writes.iter().rev() {
        if store.read(&write.location).ok().as_ref() == Some(&write.before) {
            continue;
        }
        let undone = match &write.before {
            Some(bytes) => store.write(&write.location, bytes),
            None => store.clear(&write.location),
        };
        restored &=
            undone.is_ok() && store.read(&write.location).ok().as_ref() == Some(&write.before);
    }
    restored
}

/// True when every store still holds what the switch wrote.
pub fn verify(store: &dyn CredentialStore, record: &SwitchRecord) -> bool {
    record.items.iter().all(|item| {
        store
            .read(&item.location)
            .is_ok_and(|now| item.location.fingerprint(now.as_deref()) == item.after)
    })
}

/// What Settings shows when a switch failed verification or didn't finish.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOffer {
    pub id: String,
    pub provider: Provider,
    pub reason: &'static str,
    pub created_at: String,
}

impl RestoreOffer {
    pub fn new(record: &SwitchRecord, now: chrono::DateTime<chrono::Utc>) -> Option<Self> {
        record.restorable(now).then(|| Self {
            id: record.id.clone(),
            provider: record.provider,
            reason: match record.state {
                RecordState::VerifyFailed => "verifyFailed",
                RecordState::RollbackFailed => "rollbackFailed",
                _ => "unfinished",
            },
            created_at: record.created_at.clone(),
        })
    }
}

pub struct Recovery {
    pub offer: Option<SwitchRecord>,
    pub apply: Vec<SwitchRecord>,
}

/// At launch: finish switches that wrote everything but not Margin's model, and
/// offer a restore for any that stopped part way or failed verification.
pub fn recover(
    store: &dyn CredentialStore,
    journal: &mut dyn Journal,
    now: chrono::DateTime<chrono::Utc>,
) -> Recovery {
    let mut recovery = Recovery {
        offer: None,
        apply: Vec::new(),
    };
    let mut records = journal.records();
    records.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    for record in records {
        match record.state {
            RecordState::Started => {
                let current: Option<Vec<Option<String>>> = record
                    .items
                    .iter()
                    .map(|item| {
                        store
                            .read(&item.location)
                            .ok()
                            .map(|bytes| item.location.fingerprint(bytes.as_deref()))
                    })
                    .collect();
                let matches = |side: fn(&RecordItem) -> &Option<String>| {
                    current.as_ref().is_some_and(|current| {
                        current
                            .iter()
                            .zip(&record.items)
                            .all(|(current, item)| current == side(item))
                    })
                };
                if matches(|item| &item.after) {
                    let _ = journal.mark(&record.id, RecordState::Written);
                    recovery.apply.push(record);
                } else if matches(|item| &item.before) {
                    let _ = journal.mark(&record.id, RecordState::Abandoned);
                } else {
                    let live = fingerprints(store, &record.items);
                    let _ = journal.fail(&record.id, RecordState::Started, live.clone());
                    let record = SwitchRecord {
                        live: Some(live),
                        ..record
                    };
                    if record.restorable(now) {
                        recovery.offer = Some(record);
                    }
                }
            }
            RecordState::Written => recovery.apply.push(record),
            RecordState::VerifyFailed | RecordState::RollbackFailed if record.restorable(now) => {
                recovery.offer = Some(record)
            }
            _ => {}
        }
    }
    recovery
}

/// The account record follows the person: sources trade places, so id, label,
/// pin, alerts, snapshot and history stay with the identity.
pub fn apply_to_accounts(accounts: &mut [Account], moved: &RecordAccounts) -> bool {
    let Some(target) = accounts
        .iter()
        .position(|account| account.id == moved.target && account.source == moved.target_source)
    else {
        return false;
    };
    let previous = moved.previous.as_ref().and_then(|id| {
        accounts
            .iter()
            .position(|account| &account.id == id && account.source == moved.default_source)
    });
    relocate(&mut accounts[target], &moved.default_source);
    if let Some(previous) = previous {
        relocate(&mut accounts[previous], &moved.target_source);
    }
    true
}

/// Reverses `apply_to_accounts` after a restore put the old sign-ins back.
pub fn undo_accounts(accounts: &mut [Account], moved: &RecordAccounts) -> bool {
    apply_to_accounts(
        accounts,
        &RecordAccounts {
            target: moved.target.clone(),
            previous: moved.previous.clone(),
            target_source: moved.default_source.clone(),
            default_source: moved.target_source.clone(),
        },
    )
}

fn relocate(account: &mut Account, source: &AccountSource) {
    if account.label == default_label(account.provider, &account.source) {
        account.label = default_label(account.provider, source);
    }
    account.source = source.clone();
}

fn default_label(provider: Provider, source: &AccountSource) -> String {
    match source {
        AccountSource::ConfigDir { path, .. } => accounts::default_label(provider, Path::new(path)),
        AccountSource::Keychain => provider.name().into(),
    }
}

/// Journal index in `switches.json` next to Margin's settings (no secrets);
/// backups in a Margin-owned Keychain item on macOS, else a 0600 file.
pub struct DiskJournal {
    pub dir: PathBuf,
    pub keychain: bool,
}

impl DiskJournal {
    fn index(&self) -> PathBuf {
        self.dir.join("switches.json")
    }

    fn backup_file(&self, id: &str) -> PathBuf {
        self.dir.join("backups").join(format!("{id}.json"))
    }

    fn save(&self, records: &[SwitchRecord]) -> Result<(), SwitchError> {
        std::fs::create_dir_all(&self.dir).map_err(|_| SwitchError::Journal)?;
        let bytes = serde_json::to_vec_pretty(records).map_err(|_| SwitchError::Journal)?;
        write_file(&self.index(), &bytes).map_err(|_| SwitchError::Journal)
    }

    fn store_backup(&self, id: &str, bytes: &[u8]) -> Result<(), SwitchError> {
        if self.keychain {
            let chunks = backup_chunks(bytes).ok_or(SwitchError::Journal)?;
            for (index, chunk) in chunks.iter().enumerate() {
                if write_keychain(BACKUP_SERVICE, &format!("{id}.{index}"), chunk.as_bytes())
                    .is_err()
                {
                    self.drop_backup(id);
                    return Err(SwitchError::Journal);
                }
            }
            return Ok(());
        }
        let path = self.backup_file(id);
        let folder = path.parent().ok_or(SwitchError::Journal)?;
        std::fs::create_dir_all(folder).map_err(|_| SwitchError::Journal)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| SwitchError::Journal)?;
        }
        write_file(&path, bytes).map_err(|_| SwitchError::Journal)
    }

    fn drop_backup(&self, id: &str) {
        if self.keychain {
            for index in 0..MAX_BACKUP_CHUNKS {
                let account = format!("{id}.{index}");
                if read_keychain_account(BACKUP_SERVICE, &account)
                    .ok()
                    .flatten()
                    .is_none()
                {
                    break;
                }
                let _ = remove_keychain(BACKUP_SERVICE, &account);
            }
        } else {
            let _ = std::fs::remove_file(self.backup_file(id));
        }
    }
}

impl Journal for DiskJournal {
    fn records(&self) -> Vec<SwitchRecord> {
        read_file(&self.index())
            .ok()
            .flatten()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn begin(&mut self, record: &SwitchRecord, backup: &[u8]) -> Result<(), SwitchError> {
        self.store_backup(&record.id, backup)?;
        let mut records = self.records();
        records.push(record.clone());
        let now = chrono::Utc::now();
        let cutoff = now - chrono::Duration::days(KEEP_DAYS);
        let keep_from = records.len().saturating_sub(KEEP_RECORDS);
        let (kept, dropped): (Vec<_>, Vec<_>) =
            records.into_iter().enumerate().partition(|(index, entry)| {
                entry.id == record.id
                    || entry.restorable(now)
                    || (*index >= keep_from
                        && chrono::DateTime::parse_from_rfc3339(&entry.created_at)
                            .is_ok_and(|at| at > cutoff))
            });
        let kept: Vec<SwitchRecord> = kept.into_iter().map(|(_, entry)| entry).collect();
        if let Err(error) = self.save(&kept) {
            self.drop_backup(&record.id);
            return Err(error);
        }
        for (_, entry) in dropped {
            self.drop_backup(&entry.id);
        }
        Ok(())
    }

    fn mark(&mut self, id: &str, state: RecordState) -> Result<(), SwitchError> {
        let mut records = self.records();
        let record = records
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or(SwitchError::Journal)?;
        record.state = state;
        self.save(&records)
    }

    fn fail(
        &mut self,
        id: &str,
        state: RecordState,
        live: Vec<Option<String>>,
    ) -> Result<(), SwitchError> {
        let mut records = self.records();
        let record = records
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or(SwitchError::Journal)?;
        record.state = state;
        record.live = Some(live);
        self.save(&records)
    }

    fn backup(&self, id: &str) -> Result<Option<Vec<u8>>, SwitchError> {
        if self.keychain {
            let mut chunks = Vec::new();
            for index in 0..MAX_BACKUP_CHUNKS {
                match read_keychain_account(BACKUP_SERVICE, &format!("{id}.{index}"))? {
                    Some(chunk) => chunks.push(chunk),
                    None => break,
                }
            }
            return Ok((!chunks.is_empty()).then(|| join_chunks(&chunks)).flatten());
        }
        read_file(&self.backup_file(id)).map_err(|_| SwitchError::Journal)
    }
}

/// Keychain backups are base64 in chunks that fit one `security -i` line, so the
/// secret never needs argv and survives `-w` printing only plain text.
fn backup_chunks(bytes: &[u8]) -> Option<Vec<String>> {
    let encoded = STANDARD.encode(bytes);
    let chunks: Vec<String> = encoded
        .as_bytes()
        .chunks(BACKUP_CHUNK)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect();
    (chunks.len() <= MAX_BACKUP_CHUNKS).then_some(chunks)
}

fn join_chunks(chunks: &[Vec<u8>]) -> Option<Vec<u8>> {
    STANDARD.decode(chunks.concat()).ok()
}

fn read_keychain_account(service: &str, account: &str) -> Result<Option<Vec<u8>>, SwitchError> {
    #[cfg(target_os = "macos")]
    {
        keychain::read_account(service, account).map_err(|_| SwitchError::Journal)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (service, account);
        Err(SwitchError::Journal)
    }
}

#[cfg(test)]
mod tests;
