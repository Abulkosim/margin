use super::*;
use crate::core::history::HistoryStore;
use crate::core::model::{Identity, Snapshot, UsageWindow};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    Keychain(String),
    Path(PathBuf),
}

fn key(location: &Location) -> Key {
    match location {
        Location::Keychain { service, .. } => Key::Keychain(service.clone()),
        Location::File { path } | Location::Config { path } => Key::Path(path.clone()),
    }
}

#[derive(Clone, Copy)]
struct Failure {
    at: usize,
    after_write: bool,
    keychain: bool,
    sticky: bool,
}

type Hook = Box<dyn FnMut(usize, &mut HashMap<Key, Vec<u8>>)>;

#[derive(Default)]
struct FakeStore {
    values: RefCell<HashMap<Key, Vec<u8>>>,
    accounts: HashMap<String, String>,
    symlinks: HashSet<PathBuf>,
    mutations: Cell<usize>,
    fail: Cell<Option<Failure>>,
    hook: RefCell<Option<Hook>>,
}

impl FakeStore {
    fn set(&self, location: &Location, value: &str) {
        self.values
            .borrow_mut()
            .insert(key(location), value.as_bytes().to_vec());
    }

    fn get(&self, location: &Location) -> Option<String> {
        self.values
            .borrow()
            .get(&key(location))
            .map(|bytes| String::from_utf8(bytes.clone()).unwrap())
    }

    fn snapshot(&self) -> HashMap<Key, Vec<u8>> {
        self.values.borrow().clone()
    }

    fn mutate(&self, change: impl FnOnce(&mut HashMap<Key, Vec<u8>>)) -> Result<(), StoreError> {
        let index = self.mutations.get();
        self.mutations.set(index + 1);
        let failure = self
            .fail
            .get()
            .filter(|failure| index == failure.at || (failure.sticky && index > failure.at));
        if failure.is_none_or(|failure| failure.after_write) {
            change(&mut self.values.borrow_mut());
        }
        if let Some(failure) = failure {
            return Err(if failure.keychain {
                StoreError::Keychain
            } else {
                StoreError::Io
            });
        }
        if let Some(hook) = self.hook.borrow_mut().as_mut() {
            hook(index, &mut self.values.borrow_mut());
        }
        Ok(())
    }
}

impl CredentialStore for FakeStore {
    fn read(&self, location: &Location) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.values.borrow().get(&key(location)).cloned())
    }

    fn write(&self, location: &Location, bytes: &[u8]) -> Result<(), StoreError> {
        self.mutate(|values| {
            values.insert(key(location), bytes.to_vec());
        })
    }

    fn clear(&self, location: &Location) -> Result<(), StoreError> {
        self.mutate(|values| {
            values.remove(&key(location));
        })
    }

    fn keychain_account(&self, service: &str) -> Result<Option<String>, StoreError> {
        Ok(self.accounts.get(service).cloned())
    }

    fn is_symlink(&self, path: &Path) -> bool {
        self.symlinks.contains(path)
    }
}

#[derive(Default)]
struct FakeLocks(RefCell<Vec<PathBuf>>);

impl LockSet for FakeLocks {
    fn acquire(&self, paths: &[PathBuf]) -> Result<lock::Held, LockError> {
        self.0.borrow_mut().extend_from_slice(paths);
        Ok(lock::Held::default())
    }
}

#[derive(Default)]
struct FakeJournal {
    records: Vec<SwitchRecord>,
    backups: HashMap<String, Vec<u8>>,
    fail_begin: bool,
}

impl Journal for FakeJournal {
    fn records(&self) -> Vec<SwitchRecord> {
        self.records.clone()
    }

    fn begin(&mut self, record: &SwitchRecord, backup: &[u8]) -> Result<(), SwitchError> {
        if self.fail_begin {
            return Err(SwitchError::Journal);
        }
        self.records.push(record.clone());
        self.backups.insert(record.id.clone(), backup.to_vec());
        Ok(())
    }

    fn mark(&mut self, id: &str, state: RecordState) -> Result<(), SwitchError> {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or(SwitchError::Journal)?;
        record.state = state;
        Ok(())
    }

    fn backup(&self, id: &str) -> Result<Option<Vec<u8>>, SwitchError> {
        Ok(self.backups.get(id).cloned())
    }
}

fn home() -> PathBuf {
    PathBuf::from("/margin-test/home")
}

fn work_dir() -> String {
    home().join(".claude-work").to_string_lossy().into_owned()
}

fn credential(tag: &str) -> String {
    format!(
        r#"{{"claudeAiOauth":{{"accessToken":"secret-access-{tag}","refreshToken":"secret-refresh-{tag}","expiresAt":1,"scopes":["user:profile"]}},"mcpOAuth":{{}}}}"#
    )
}

fn config(tag: &str) -> String {
    format!(
        "{{\n  \"numStartups\": 3,\n  \"userID\": \"device-{tag}\",\n  \"oauthAccount\": {{\n    \"accountUuid\": \"{tag}-uuid\",\n    \"organizationUuid\": \"org\",\n    \"emailAddress\": \"{tag}@example.com\"\n  }},\n  \"projects\": {{}}\n}}\n"
    )
}

struct Fixture {
    store: FakeStore,
    default: SlotSpec,
    source: SlotSpec,
}

impl Fixture {
    fn claude(default_source: AccountSource) -> Self {
        let mut store = FakeStore::default();
        store
            .accounts
            .insert("Claude Code-credentials".into(), "tester".into());
        let home = home();
        let (default, source) = {
            let context = Context {
                home: &home,
                keychain: true,
                store: &store,
            };
            (
                slot_spec(Provider::Claude, &default_source, &context).unwrap(),
                slot_spec(
                    Provider::Claude,
                    &AccountSource::ConfigDir {
                        path: work_dir(),
                        keychain_suffix: None,
                    },
                    &context,
                )
                .unwrap(),
            )
        };
        store.set(&default.credentials[0], &credential("personal"));
        store.set(&source.credentials[0], &credential("work"));
        store.set(&default.config_location(), &config("personal"));
        store.set(&source.config_location(), &config("work"));
        Self {
            store,
            default,
            source,
        }
    }

    fn default_claude() -> Self {
        Self::claude(AccountSource::ConfigDir {
            path: home().join(".claude").to_string_lossy().into_owned(),
            keychain_suffix: None,
        })
    }

    fn states(&self) -> (SlotState, SlotState) {
        (
            read_slot(&self.default, &self.store).unwrap(),
            read_slot(&self.source, &self.store).unwrap(),
        )
    }

    fn plan(&self) -> Result<Plan, Refusal> {
        let (default, source) = self.states();
        plan(&default, &source)
    }

    fn execute(&self, journal: &mut dyn Journal) -> Result<Outcome, SwitchError> {
        let fingerprint = self.plan().unwrap().fingerprint;
        execute(
            &fingerprint,
            &self.default,
            &self.source,
            None,
            &self.store,
            &FakeLocks::default(),
            journal,
        )
    }
}

impl SlotSpec {
    fn config_location(&self) -> Location {
        Location::Config {
            path: self.config.clone().unwrap(),
        }
    }
}

fn codex_token(claims: Value) -> String {
    format!(
        "e30.{}.sig",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    )
}

fn codex_auth(user: &str, refresh: bool) -> String {
    let token = codex_token(serde_json::json!({
        "email": format!("{user}@example.com"),
        "https://api.openai.com/auth": { "chatgpt_account_id": "workspace", "chatgpt_user_id": user }
    }));
    serde_json::json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "id_token": token,
            "access_token": format!("secret-access-{user}"),
            "refresh_token": if refresh { format!("secret-refresh-{user}") } else { String::new() },
            "account_id": "workspace"
        },
        "last_refresh": "2026-10-20T10:00:00Z"
    })
    .to_string()
}

fn codex_fixture() -> Fixture {
    let store = FakeStore::default();
    let home = home();
    let (default, source) = {
        let context = Context {
            home: &home,
            keychain: false,
            store: &store,
        };
        let spec = |path: PathBuf| {
            slot_spec(
                Provider::Codex,
                &AccountSource::ConfigDir {
                    path: path.to_string_lossy().into_owned(),
                    keychain_suffix: None,
                },
                &context,
            )
            .unwrap()
        };
        (spec(home.join(".codex")), spec(home.join(".codex-work")))
    };
    store.set(&default.credentials[0], &codex_auth("personal", true));
    store.set(&source.credentials[0], &codex_auth("work", true));
    Fixture {
        store,
        default,
        source,
    }
}

#[test]
fn claude_specs_use_claude_codes_paths_locks_and_keychain_precedence() {
    let fixture = Fixture::default_claude();
    let default = &fixture.default;
    assert!(default.default && !fixture.source.default);
    assert_eq!(
        default.credentials,
        [
            Location::Keychain {
                service: "Claude Code-credentials".into(),
                account: "tester".into()
            },
            Location::File {
                path: home().join(".claude/.credentials.json")
            }
        ]
    );
    assert_eq!(default.config, Some(home().join(".claude.json")));
    assert_eq!(
        default.locks,
        [
            home().join(".claude/.oauth_refresh.lock"),
            home().join(".claude.lock"),
            home().join(".claude/.storage-write.lock"),
            home().join(".claude.json.lock"),
        ]
    );
    let Location::Keychain { service, account } = &fixture.source.credentials[0] else {
        panic!("keychain first");
    };
    assert_eq!(
        service,
        &format!("Claude Code-credentials-{}", keychain_suffix(&work_dir()))
    );
    assert_eq!(account, &keychain::default_account());
    assert_eq!(
        fixture.source.config,
        Some(home().join(".claude-work/.claude.json"))
    );

    fixture
        .store
        .set(&fixture.source.credentials[1], &credential("stale-file"));
    let (_, source) = fixture.states();
    assert_eq!(
        source.credential.unwrap().0,
        fixture.source.credentials[0],
        "Keychain wins over the plaintext file, like Claude Code"
    );
}

#[test]
fn legacy_config_json_wins_when_present() {
    let store = FakeStore::default();
    let home = home();
    store.set(
        &Location::Config {
            path: home.join(".claude-work/.config.json"),
        },
        "{}",
    );
    let context = Context {
        home: &home,
        keychain: false,
        store: &store,
    };
    let spec = slot_spec(
        Provider::Claude,
        &AccountSource::ConfigDir {
            path: work_dir(),
            keychain_suffix: None,
        },
        &context,
    )
    .unwrap();
    assert_eq!(spec.config, Some(home.join(".claude-work/.config.json")));
    assert_eq!(
        spec.credentials,
        [Location::File {
            path: home.join(".claude-work/.credentials.json")
        }]
    );
}

#[test]
fn plan_table() {
    let swap = Fixture::default_claude().plan().unwrap();
    assert_eq!(swap.kind, PlanKind::Swap);
    assert_eq!(swap.target_email.as_deref(), Some("work@example.com"));
    assert_eq!(
        swap.displaced_email.as_deref(),
        Some("personal@example.com")
    );

    let fixture = Fixture::default_claude();
    fixture
        .store
        .values
        .borrow_mut()
        .remove(&key(&fixture.default.credentials[0]));
    let moved = fixture.plan().unwrap();
    assert_eq!(moved.kind, PlanKind::Move);
    assert_eq!(moved.displaced_email, None);

    let fixture = Fixture::default_claude();
    fixture
        .store
        .set(&fixture.source.config_location(), &config("personal"));
    assert_eq!(fixture.plan(), Err(Refusal::SameAccount));

    let fixture = Fixture::default_claude();
    fixture
        .store
        .set(&fixture.source.config_location(), "{\"numStartups\": 1}");
    assert_eq!(fixture.plan(), Err(Refusal::SourceSignedOut));

    let fixture = Fixture::default_claude();
    fixture.store.set(
        &fixture.source.credentials[0],
        r#"{"claudeAiOauth":{"accessToken":"a","refreshToken":"r","expiresAt":1000}}"#,
    );
    assert_eq!(
        fixture.plan().unwrap().kind,
        PlanKind::Swap,
        "expired with a refresh token"
    );

    let fixture = Fixture::default_claude();
    fixture.store.set(
        &fixture.source.credentials[0],
        r#"{"claudeAiOauth":{"accessToken":"a"}}"#,
    );
    assert_eq!(fixture.plan(), Err(Refusal::SourceSignedOut));

    let fixture = Fixture::default_claude();
    fixture
        .store
        .set(&fixture.source.credentials[0], r#"{"mcpOAuth":{}}"#);
    assert_eq!(fixture.plan(), Err(Refusal::SourceSignedOut));

    let fixture = Fixture::default_claude();
    fixture.store.set(
        &fixture.default.credentials[0],
        r#"{"claudeAiOauth":{"accessToken":"a"}}"#,
    );
    assert_eq!(fixture.plan(), Err(Refusal::DefaultSignedOut));

    let fixture = Fixture::default_claude();
    fixture
        .store
        .set(&fixture.default.config_location(), "{\"broken\":");
    assert_eq!(fixture.plan(), Err(Refusal::ConfigUnreadable));

    let fixture = Fixture::default_claude();
    fixture
        .store
        .values
        .borrow_mut()
        .remove(&key(&fixture.source.config_location()));
    assert_eq!(
        fixture.plan(),
        Err(Refusal::ConfigMissing(
            home().join(".claude-work/.claude.json")
        ))
    );

    let mut fixture = Fixture::default_claude();
    let link = home().join(".claude-work/.credentials.json");
    fixture.store.symlinks.insert(link.clone());
    assert_eq!(fixture.plan(), Err(Refusal::Symlink(link)));

    let keychain = Fixture::claude(AccountSource::Keychain);
    assert!(keychain.default.default);
    assert_eq!(keychain.plan().unwrap().kind, PlanKind::Swap);

    let fixture = Fixture::default_claude();
    let (default, source) = fixture.states();
    assert_eq!(plan(&source, &default), Err(Refusal::NotOffered));
    assert_eq!(plan(&default, &default), Err(Refusal::NotOffered));
}

#[test]
fn codex_plan_table() {
    let fixture = codex_fixture();
    let plan = fixture.plan().unwrap();
    assert_eq!(plan.kind, PlanKind::Swap);
    assert_eq!(plan.target_email.as_deref(), Some("work@example.com"));

    for mode in ["keyring", "auto"] {
        let fixture = codex_fixture();
        fixture.store.set(
            &Location::File {
                path: home().join(".codex-work/config.toml"),
            },
            &format!("model = \"x\"\ncli_auth_credentials_store = \"{mode}\"\n"),
        );
        assert_eq!(fixture.plan(), Err(Refusal::CodexKeyring), "{mode}");
    }
    let fixture = codex_fixture();
    fixture.store.set(
        &Location::File {
            path: home().join(".codex/config.toml"),
        },
        "cli_auth_credentials_store = \"file\"\n",
    );
    assert!(fixture.plan().is_ok());
    fixture.store.set(
        &Location::File {
            path: home().join(".codex/config.toml"),
        },
        "not = = toml",
    );
    assert_eq!(
        fixture.plan(),
        Err(Refusal::CodexConfig(home().join(".codex")))
    );

    let fixture = codex_fixture();
    fixture.store.set(
        &fixture.source.credentials[0],
        &codex_auth("personal", true),
    );
    assert_eq!(fixture.plan(), Err(Refusal::SameAccount));

    let fixture = codex_fixture();
    fixture
        .store
        .set(&fixture.source.credentials[0], &codex_auth("work", false));
    assert_eq!(fixture.plan(), Err(Refusal::SourceSignedOut));

    let fixture = codex_fixture();
    fixture.store.set(
        &fixture.source.credentials[0],
        r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-x"}"#,
    );
    assert_eq!(fixture.plan(), Err(Refusal::ApiKey));

    let fixture = codex_fixture();
    fixture
        .store
        .values
        .borrow_mut()
        .remove(&key(&fixture.default.credentials[0]));
    assert_eq!(fixture.plan().unwrap().kind, PlanKind::Move);
}

fn family_locations(fixture: &Fixture) -> Vec<Option<String>> {
    [
        &fixture.default.credentials[0],
        &fixture.default.credentials[1],
        &fixture.source.credentials[0],
        &fixture.source.credentials[1],
    ]
    .into_iter()
    .map(|location| fixture.store.get(location))
    .collect()
}

#[test]
fn swap_moves_each_family_to_exactly_one_place_and_keeps_other_config_keys() {
    let fixture = Fixture::default_claude();
    let mut journal = FakeJournal::default();
    let locks = FakeLocks::default();
    let fingerprint = fixture.plan().unwrap().fingerprint;
    let outcome = execute(
        &fingerprint,
        &fixture.default,
        &fixture.source,
        None,
        &fixture.store,
        &locks,
        &mut journal,
    )
    .unwrap();
    assert_eq!(outcome.plan.kind, PlanKind::Swap);
    assert_eq!(
        family_locations(&fixture),
        [
            Some(credential("work")),
            None,
            Some(credential("personal")),
            None
        ]
    );
    let default_config = fixture
        .store
        .get(&fixture.default.config_location())
        .unwrap();
    assert_eq!(
        default_config,
        config("personal").replace(
            splice::raw_value(&config("personal"), OAUTH_ACCOUNT)
                .unwrap()
                .unwrap(),
            splice::raw_value(&config("work"), OAUTH_ACCOUNT)
                .unwrap()
                .unwrap()
        )
    );
    assert!(
        default_config.contains("device-personal"),
        "userID stays with the folder"
    );
    assert!(fixture
        .store
        .get(&fixture.source.config_location())
        .unwrap()
        .contains("personal@example.com"));
    assert_eq!(journal.records[0].state, RecordState::Written);
    assert_eq!(locks.0.borrow().len(), 8);
    let blob: BackupBlob =
        serde_json::from_slice(&journal.backups[&journal.records[0].id]).unwrap();
    assert_eq!(blob.items.len(), 4);
    assert_eq!(
        STANDARD
            .decode(blob.items[0].value.as_ref().unwrap())
            .unwrap(),
        credential("personal").into_bytes()
    );
    assert!(blob.items[2]
        .value
        .as_ref()
        .unwrap()
        .contains("personal-uuid"));

    let back = Fixture {
        store: fixture.store,
        default: fixture.default.clone(),
        source: fixture.source.clone(),
    };
    back.execute(&mut journal).unwrap();
    assert_eq!(
        back.store.get(&back.default.config_location()).unwrap(),
        config("personal"),
        "undo restores the file byte for byte"
    );
    assert_eq!(
        back.store.get(&back.default.credentials[0]),
        Some(credential("personal"))
    );
}

#[test]
fn move_signs_out_the_source_when_the_default_slot_is_empty() {
    let fixture = Fixture::default_claude();
    fixture
        .store
        .values
        .borrow_mut()
        .remove(&key(&fixture.default.credentials[0]));
    fixture.execute(&mut FakeJournal::default()).unwrap();
    assert_eq!(
        family_locations(&fixture),
        [Some(credential("work")), None, None, None]
    );
    let source_config = fixture
        .store
        .get(&fixture.source.config_location())
        .unwrap();
    assert!(!source_config.contains("oauthAccount"));
    assert!(source_config.contains("device-work"));
}

#[test]
fn a_refresh_between_preview_and_lock_uses_the_fresh_value() {
    let fixture = Fixture::default_claude();
    let fingerprint = fixture.plan().unwrap().fingerprint;
    fixture.store.set(
        &fixture.default.credentials[0],
        &credential("personal-refreshed"),
    );
    execute(
        &fingerprint,
        &fixture.default,
        &fixture.source,
        None,
        &fixture.store,
        &FakeLocks::default(),
        &mut FakeJournal::default(),
    )
    .unwrap();
    assert_eq!(
        fixture.store.get(&fixture.source.credentials[0]),
        Some(credential("personal-refreshed"))
    );

    let fixture = Fixture::default_claude();
    let fingerprint = fixture.plan().unwrap().fingerprint;
    fixture
        .store
        .set(&fixture.source.config_location(), &config("someone-else"));
    let before = fixture.store.snapshot();
    let mut journal = FakeJournal::default();
    assert_eq!(
        execute(
            &fingerprint,
            &fixture.default,
            &fixture.source,
            None,
            &fixture.store,
            &FakeLocks::default(),
            &mut journal,
        )
        .err(),
        Some(SwitchError::Changed)
    );
    assert_eq!(fixture.store.snapshot(), before);
    assert!(journal.records.is_empty());
}

#[test]
fn a_failure_at_any_write_rolls_everything_back() {
    for provider in [Provider::Claude, Provider::Codex] {
        let count = if provider == Provider::Claude { 4 } else { 2 };
        for at in 0..count {
            for after_write in [false, true] {
                let fixture = match provider {
                    Provider::Claude => Fixture::default_claude(),
                    Provider::Codex => codex_fixture(),
                };
                let before = fixture.store.snapshot();
                fixture.store.fail.set(Some(Failure {
                    at,
                    after_write,
                    keychain: false,
                    sticky: false,
                }));
                let mut journal = FakeJournal::default();
                assert_eq!(
                    fixture.execute(&mut journal).err(),
                    Some(SwitchError::RolledBack { keychain: false }),
                    "{provider:?} write {at}"
                );
                assert_eq!(fixture.store.snapshot(), before, "{provider:?} write {at}");
                assert_eq!(journal.records[0].state, RecordState::RolledBack);
            }
        }
    }
}

#[test]
fn keychain_denial_and_failed_rollback_are_reported_distinctly() {
    let fixture = Fixture::default_claude();
    let before = fixture.store.snapshot();
    fixture.store.fail.set(Some(Failure {
        at: 0,
        after_write: false,
        keychain: true,
        sticky: false,
    }));
    let error = fixture.execute(&mut FakeJournal::default()).unwrap_err();
    assert_eq!(error, SwitchError::RolledBack { keychain: true });
    assert_eq!(fixture.store.snapshot(), before);
    assert_eq!(
        message(&error, Provider::Claude, "Work", Path::new("/x")),
        "Keychain access was denied. Unlock it and try again."
    );

    let fixture = Fixture::default_claude();
    fixture.store.fail.set(Some(Failure {
        at: 2,
        after_write: true,
        keychain: false,
        sticky: true,
    }));
    let mut journal = FakeJournal::default();
    let error = fixture.execute(&mut journal).unwrap_err();
    assert!(
        matches!(error, SwitchError::RollbackFailed { ref id } if *id == journal.records[0].id)
    );
    assert_eq!(journal.records[0].state, RecordState::RollbackFailed);
    assert!(journal.backups.contains_key(&journal.records[0].id));
}

#[test]
fn nothing_is_written_without_a_backup() {
    let fixture = Fixture::default_claude();
    let before = fixture.store.snapshot();
    let mut journal = FakeJournal {
        fail_begin: true,
        ..FakeJournal::default()
    };
    assert_eq!(
        fixture.execute(&mut journal).err(),
        Some(SwitchError::Journal)
    );
    assert_eq!(fixture.store.snapshot(), before);
    assert_eq!(fixture.store.mutations.get(), 0);
}

#[test]
fn a_cli_write_during_the_switch_fails_verification_without_rollback() {
    let fixture = Fixture::default_claude();
    let target = key(&fixture.default.credentials[0]);
    *fixture.store.hook.borrow_mut() = Some(Box::new(move |index, values| {
        if index == 3 {
            values.insert(target.clone(), credential("cli-wrote").into_bytes());
        }
    }));
    let mut journal = FakeJournal::default();
    let error = fixture.execute(&mut journal).unwrap_err();
    assert!(matches!(error, SwitchError::VerifyFailed { .. }));
    assert_eq!(journal.records[0].state, RecordState::VerifyFailed);
    assert_eq!(
        fixture.store.get(&fixture.default.credentials[0]),
        Some(credential("cli-wrote")),
        "the CLI's newer token is never overwritten"
    );
    assert_eq!(
        message(&error, Provider::Claude, "Work", Path::new("/x")),
        "Claude Code changed the sign-in during the switch."
    );
}

#[test]
fn a_second_verify_catches_a_later_codex_write() {
    let fixture = codex_fixture();
    let outcome = fixture.execute(&mut FakeJournal::default()).unwrap();
    assert!(verify(&fixture.store, &outcome.record));
    fixture.store.set(
        &fixture.default.credentials[0],
        &codex_auth("personal", true),
    );
    assert!(!verify(&fixture.store, &outcome.record));
}

#[test]
fn unparsable_config_means_no_write() {
    let fixture = Fixture::default_claude();
    let fingerprint = fixture.plan().unwrap().fingerprint;
    fixture
        .store
        .set(&fixture.source.config_location(), "{\"a\":1,\"a\":2}");
    assert_eq!(
        execute(
            &fingerprint,
            &fixture.default,
            &fixture.source,
            None,
            &fixture.store,
            &FakeLocks::default(),
            &mut FakeJournal::default(),
        )
        .err(),
        Some(SwitchError::Refused(Refusal::ConfigUnreadable))
    );
    assert_eq!(fixture.store.mutations.get(), 0);
}

#[test]
fn a_held_claude_lock_aborts_before_anything_is_written() {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::default_claude();
    fixture.default.locks = vec![
        directory.path().join(".claude/.oauth_refresh.lock"),
        directory.path().join(".claude.json.lock"),
    ];
    fixture.source.locks = vec![directory.path().join(".claude-work/.oauth_refresh.lock")];
    std::fs::create_dir_all(directory.path().join(".claude")).unwrap();
    std::fs::create_dir_all(directory.path().join(".claude-work")).unwrap();
    let held = directory.path().join(".claude.json.lock");
    std::fs::create_dir(&held).unwrap();
    let locks = ProperLockfile {
        timeout: Duration::from_millis(200),
    };
    let fingerprint = fixture.plan().unwrap().fingerprint;
    let before = fixture.store.snapshot();
    let mut journal = FakeJournal::default();
    let error = execute(
        &fingerprint,
        &fixture.default,
        &fixture.source,
        None,
        &fixture.store,
        &locks,
        &mut journal,
    )
    .unwrap_err();
    assert_eq!(error, SwitchError::Busy);
    assert_eq!(
        message(&error, Provider::Claude, "Work", Path::new("/x")),
        "Claude Code is busy. Try again in a moment."
    );
    assert_eq!(fixture.store.snapshot(), before);
    assert!(journal.records.is_empty());
    assert!(held.is_dir());
    assert!(!directory
        .path()
        .join(".claude/.oauth_refresh.lock")
        .exists());

    std::fs::remove_dir(&held).unwrap();
    fixture.store.fail.set(Some(Failure {
        at: 1,
        after_write: false,
        keychain: false,
        sticky: false,
    }));
    assert!(execute(
        &fingerprint,
        &fixture.default,
        &fixture.source,
        None,
        &fixture.store,
        &locks,
        &mut journal,
    )
    .is_err());
    for lock in fixture.default.locks.iter().chain(&fixture.source.locks) {
        assert!(
            !lock.exists(),
            "released after an error: {}",
            lock.display()
        );
    }
    fixture.store.fail.set(None);
    execute(
        &fingerprint,
        &fixture.default,
        &fixture.source,
        None,
        &fixture.store,
        &locks,
        &mut journal,
    )
    .unwrap();
    for lock in fixture.default.locks.iter().chain(&fixture.source.locks) {
        assert!(!lock.exists(), "released after success: {}", lock.display());
    }
}

#[test]
fn journal_recovery_detects_unfinished_switches() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::default_claude();
    let (default, source) = fixture.states();
    let plan = plan(&default, &source).unwrap();
    let pending = writes(&plan, &default, &source).unwrap();
    let mut journal = DiskJournal {
        dir: directory.path().into(),
        keychain: false,
    };
    let record = new_record(Provider::Claude, false, &pending, Vec::new());
    journal
        .begin(&record, &backup_blob(&record.id, &pending).unwrap())
        .unwrap();

    let recovery = recover(&fixture.store, &mut journal);
    assert!(recovery.offer.is_none() && recovery.apply.is_empty());
    assert_eq!(journal.records()[0].state, RecordState::Abandoned);

    journal.mark(&record.id, RecordState::Started).unwrap();
    fixture
        .store
        .write(&pending[0].location, pending[0].after.as_ref().unwrap())
        .unwrap();
    let recovery = recover(&fixture.store, &mut journal);
    assert_eq!(
        recovery.offer.map(|record| record.id),
        Some(record.id.clone())
    );

    for write in &pending[1..] {
        match &write.after {
            Some(bytes) => fixture.store.write(&write.location, bytes).unwrap(),
            None => fixture.store.clear(&write.location).unwrap(),
        }
    }
    let recovery = recover(&fixture.store, &mut journal);
    assert!(recovery.offer.is_none());
    assert_eq!(recovery.apply.len(), 1);
    assert_eq!(journal.records()[0].state, RecordState::Written);
}

#[test]
fn restore_writes_the_backup_back_under_a_new_record() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::default_claude();
    let before = fixture.store.snapshot();
    let mut journal = DiskJournal {
        dir: directory.path().into(),
        keychain: false,
    };
    let outcome = fixture.execute(&mut journal).unwrap();
    assert_ne!(fixture.store.snapshot(), before);
    let record = restore(
        &outcome.record.id,
        &fixture.store,
        &FakeLocks::default(),
        &mut journal,
    )
    .unwrap();
    assert!(record.restore);
    assert_eq!(fixture.store.snapshot(), before);
    let records = journal.records();
    assert_eq!(records[0].state, RecordState::Restored);
    assert_eq!(records[1].state, RecordState::Written);
}

#[test]
fn disk_journal_keeps_five_recent_records_and_private_backups() {
    let directory = tempfile::tempdir().unwrap();
    let mut journal = DiskJournal {
        dir: directory.path().into(),
        keychain: false,
    };
    let mut old = new_record(Provider::Codex, false, &[], Vec::new());
    old.id = "codex-old".into();
    old.created_at = (chrono::Utc::now() - chrono::Duration::days(8)).to_rfc3339();
    journal.begin(&old, b"old").unwrap();
    let mut ids = Vec::new();
    for index in 0..6 {
        let mut record = new_record(Provider::Codex, false, &[], Vec::new());
        record.id = format!("codex-{index}");
        journal.begin(&record, b"secret-backup").unwrap();
        ids.push(record.id);
    }
    let kept: Vec<String> = journal
        .records()
        .into_iter()
        .map(|record| record.id)
        .collect();
    assert_eq!(kept, ids[1..]);
    assert!(journal.backup("codex-old").unwrap().is_none());
    assert!(journal.backup("codex-0").unwrap().is_none());
    assert_eq!(
        journal.backup("codex-5").unwrap().as_deref(),
        Some(&b"secret-backup"[..])
    );
    let index = std::fs::read_to_string(directory.path().join("switches.json")).unwrap();
    assert!(!index.contains("secret"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: PathBuf| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(directory.path().join("backups")), 0o700);
        assert_eq!(mode(directory.path().join("backups/codex-5.json")), 0o600);
        assert_eq!(mode(directory.path().join("switches.json")), 0o600);
    }
}

#[test]
fn file_store_replaces_atomically_with_private_mode_and_spots_symlinks() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("auth.json");
    let location = Location::File { path: path.clone() };
    assert_eq!(SystemStore.read(&location).unwrap(), None);
    std::fs::write(&path, "old").unwrap();
    SystemStore.write(&location, b"new").unwrap();
    assert_eq!(SystemStore.read(&location).unwrap().unwrap(), b"new");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let link = directory.path().join("link.json");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(SystemStore.is_symlink(&link));
        assert!(!SystemStore.is_symlink(&path));
        assert!(SystemStore.read(&Location::File { path: link }).is_err());
    }
    SystemStore.clear(&location).unwrap();
    SystemStore.clear(&location).unwrap();
    assert!(!path.exists());
}

fn person(id: &str, label: &str, source: AccountSource, email: &str) -> Account {
    Account {
        id: id.into(),
        provider: Provider::Claude,
        label: label.into(),
        color: "#b48265".into(),
        source,
        identity: Identity {
            email: Some(email.into()),
            plan: Some("Max".into()),
            history_key: None,
        },
        snapshot: Some(Snapshot {
            windows: vec![UsageWindow {
                id: "five_hour".into(),
                label: "Session".into(),
                used_percent: 20.0,
                resets_at: None,
                window_seconds: Some(18_000),
            }],
            fetched_at: chrono::Utc::now().to_rfc3339(),
            extra_usage: None,
            reset_grants: None,
        }),
        error: None,
        refreshing: false,
    }
}

#[test]
fn records_follow_the_person_and_keep_their_history() {
    let directory = tempfile::tempdir().unwrap();
    let default_path = directory.path().join(".claude");
    let work_path = directory.path().join(".claude-work");
    let default = AccountSource::ConfigDir {
        path: default_path.to_string_lossy().into_owned(),
        keychain_suffix: None,
    };
    let work = AccountSource::ConfigDir {
        path: work_path.to_string_lossy().into_owned(),
        keychain_suffix: Some("0123abcd".into()),
    };
    let mut accounts = vec![
        person(
            "claude-a",
            "Claude",
            default.clone(),
            "personal@example.com",
        ),
        person("claude-b", "Work", work.clone(), "work@example.com"),
        person(
            "claude-c",
            "claude-side",
            AccountSource::ConfigDir {
                path: directory
                    .path()
                    .join(".claude-side")
                    .to_string_lossy()
                    .into_owned(),
                keychain_suffix: None,
            },
            "side@example.com",
        ),
    ];
    let mut history = HistoryStore::new(directory.path().join("history"));
    let now = chrono::Utc::now();
    for account in &accounts {
        history.record(account, 60, now);
    }
    let before = accounts.clone();
    let moved = RecordAccounts {
        target: "claude-b".into(),
        previous: Some("claude-a".into()),
        target_source: work.clone(),
        default_source: default.clone(),
    };
    assert!(apply_to_accounts(&mut accounts, &moved));
    assert_eq!(accounts[1].source, default);
    assert_eq!(accounts[1].label, "Work", "custom labels follow the person");
    assert_eq!(accounts[0].source, work);
    assert_eq!(
        accounts[0].label, "claude-work",
        "folder labels stay truthful"
    );
    assert_eq!(accounts[2], before[2]);
    for (after, before) in accounts.iter().zip(&before) {
        assert_eq!(after.id, before.id);
        assert_eq!(after.identity, before.identity);
        assert_eq!(after.snapshot, before.snapshot);
    }
    for account in &accounts {
        history.relocate(account);
        assert_eq!(history.get(account, now).points.len(), 1, "{}", account.id);
    }
    assert!(
        !apply_to_accounts(&mut accounts, &moved),
        "a second apply finds nothing to move"
    );

    let mut keychain = vec![
        person(
            "claude-k",
            "Claude",
            AccountSource::Keychain,
            "personal@example.com",
        ),
        person("claude-w", "claude-work", work.clone(), "work@example.com"),
    ];
    assert!(apply_to_accounts(
        &mut keychain,
        &RecordAccounts {
            target: "claude-w".into(),
            previous: Some("claude-k".into()),
            target_source: work.clone(),
            default_source: AccountSource::Keychain,
        }
    ));
    assert_eq!(keychain[1].source, AccountSource::Keychain);
    assert_eq!(keychain[1].label, "Claude");
    assert_eq!(keychain[0].label, "claude-work");
}

#[test]
fn default_slot_detection() {
    let home = home();
    assert!(is_default_source(
        Provider::Claude,
        &AccountSource::Keychain,
        &home
    ));
    assert!(!is_default_source(
        Provider::Codex,
        &AccountSource::Keychain,
        &home
    ));
    let at = |path: &str| AccountSource::ConfigDir {
        path: home.join(path).to_string_lossy().into_owned(),
        keychain_suffix: None,
    };
    assert!(is_default_source(Provider::Claude, &at(".claude"), &home));
    assert!(is_default_source(Provider::Codex, &at(".codex"), &home));
    assert!(!is_default_source(Provider::Codex, &at(".claude"), &home));
    assert!(!is_default_source(
        Provider::Claude,
        &at(".claude-work"),
        &home
    ));
    assert_eq!(
        display_path(&home.join(".claude-work"), &home),
        format!("~{}.claude-work", std::path::MAIN_SEPARATOR)
    );
}

#[test]
fn refusals_read_plainly() {
    let folder = Path::new("/example/.claude-work");
    let text = |refusal: Refusal| {
        message(
            &SwitchError::Refused(refusal),
            Provider::Claude,
            "Work",
            folder,
        )
    };
    assert_eq!(
        text(Refusal::SameAccount),
        "Both folders hold the same account. Sign out of one first."
    );
    assert_eq!(
        message(
            &SwitchError::Refused(Refusal::CodexKeyring),
            Provider::Codex,
            "Work",
            folder
        ),
        "Codex keeps this sign-in in the system keyring. Use `codex login` to switch."
    );
    if !cfg!(windows) {
        assert_eq!(
            text(Refusal::SourceSignedOut),
            "Work is signed out. Run CLAUDE_CONFIG_DIR=/example/.claude-work claude, then try again."
        );
    }
}

#[test]
fn journal_and_messages_never_carry_secrets() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = Fixture::default_claude();
    let mut journal = DiskJournal {
        dir: directory.path().into(),
        keychain: false,
    };
    let outcome = fixture.execute(&mut journal).unwrap();
    let mut exposed = vec![
        std::fs::read_to_string(directory.path().join("switches.json")).unwrap(),
        serde_json::to_string(&outcome.record).unwrap(),
        format!("{:?}", outcome.plan),
    ];
    for error in [
        SwitchError::Busy,
        SwitchError::Changed,
        SwitchError::Journal,
        SwitchError::RollbackFailed {
            id: outcome.record.id.clone(),
        },
        SwitchError::Refused(Refusal::SourceSignedOut),
    ] {
        exposed.push(message(&error, Provider::Claude, "Work", Path::new("/x")));
        exposed.push(format!("{error:?}"));
    }
    for text in exposed {
        assert!(!text.contains("secret-"), "{text}");
    }
}

/// Only this module may write CLI credential or config files, or Keychain items.
#[test]
fn only_the_switch_module_writes_cli_files() {
    fn sources(directory: &Path, found: &mut Vec<(PathBuf, String)>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, found);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push((path.clone(), std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    sources(&root, &mut found);
    assert!(found.len() > 10);
    let switch = root.join("core").join("switch");
    let writers = [
        "fs::write",
        "File::create",
        "OpenOptions",
        ".persist(",
        "fs::rename",
        "remove_file",
        "set_permissions",
        "set_len",
    ];
    let mut checked = 0;
    for (path, source) in found {
        if path == root.join("core").join("switch.rs") || path.starts_with(&switch) {
            continue;
        }
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .unwrap_or_default();
        for keychain in ["add-generic-password", "delete-generic-password", "\"-i\""] {
            assert!(
                !production.contains(keychain),
                "{} uses {keychain}",
                path.display()
            );
        }
        let names_cli_file = [
            ".credentials.json",
            "auth.json",
            ".claude.json",
            "config.toml",
            "credential_filename",
        ]
        .iter()
        .any(|name| production.contains(name));
        if names_cli_file {
            checked += 1;
            for writer in writers {
                assert!(
                    !production.contains(writer),
                    "{} names a CLI file and uses {writer}",
                    path.display()
                );
            }
        }
    }
    assert!(checked >= 3);
}
