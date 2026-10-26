use super::model::{Account, AccountSource, Identity, Provider};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_POINTS: usize = 80_000;
const MAX_WINDOW_POINTS: usize = 10_240;
const READ_ERROR: &str = "History could not be read. Current usage is still available.";
const IDENTITY_ERROR: &str = "History is paused until account identity is available.";
const SAVE_ERROR: &str = "History could not be saved. Current usage is still available.";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPoint {
    pub window_id: String,
    pub at: String,
    pub used_percent: f64,
    pub resets_at: Option<String>,
    pub window_seconds: Option<u64>,
    pub gap_before: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountHistory {
    pub account_id: String,
    pub points: Vec<HistoryPoint>,
    pub error: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedHistory {
    version: u32,
    provider: Provider,
    source: AccountSource,
    identity: Identity,
    points: Vec<HistoryPoint>,
}

struct Entry {
    saved: SavedHistory,
    error: Option<String>,
    unreadable: bool,
    gap: bool,
    expected_at: Option<DateTime<Utc>>,
    last_observed: Option<DateTime<Utc>>,
    present: HashSet<String>,
    last_percent: HashMap<String, f64>,
    dirty: bool,
}

pub struct HistoryStore {
    directory: PathBuf,
    entries: HashMap<String, Entry>,
}

impl HistoryStore {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            entries: HashMap::new(),
        }
    }

    fn entry(&mut self, account: &Account) -> &mut Entry {
        self.entries.entry(account.id.clone()).or_insert_with(|| {
            let result = load(&self.directory.join(format!("{}.json", account.id)));
            let (mut saved, error) = match result {
                Ok(Some(saved)) => (saved, None),
                Ok(None) => (empty(account), None),
                Err(error) => (empty(account), Some(error)),
            };
            let dirty = compact(&mut saved.points);
            Entry {
                saved,
                unreadable: error.is_some(),
                error,
                gap: true,
                expected_at: None,
                last_observed: None,
                present: HashSet::new(),
                last_percent: HashMap::new(),
                dirty,
            }
        })
    }

    pub fn get(&mut self, account: &Account, now: DateTime<Utc>) -> AccountHistory {
        let entry = self.entry(account);
        let verified = reconcile(entry, account);
        entry.dirty |= prune(&mut entry.saved.points, now);
        AccountHistory {
            account_id: account.id.clone(),
            points: if verified {
                entry.saved.points.clone()
            } else {
                Vec::new()
            },
            error: entry
                .error
                .clone()
                .or_else(|| (!verified).then(|| IDENTITY_ERROR.into())),
        }
    }

    pub fn failed(&mut self, account: &Account) {
        self.entry(account).gap = true;
    }

    pub fn record(&mut self, account: &Account, interval: u64, now: DateTime<Utc>) {
        let path = self.directory.join(format!("{}.json", account.id));
        let entry = self.entry(account);
        if !reconcile(entry, account) || entry.unreadable {
            return;
        }
        let Some(snapshot) = account
            .snapshot
            .as_ref()
            .filter(|_| account.error.is_none())
        else {
            entry.gap = true;
            return;
        };
        let Some(at) = timestamp(&snapshot.fetched_at) else {
            entry.gap = true;
            return;
        };
        if (now - at).num_seconds().abs() > 30 || entry.last_observed.is_some_and(|last| at <= last)
        {
            entry.gap = true;
            return;
        }
        let gap = entry.gap || entry.expected_at.is_some_and(|expected| at > expected);
        entry.expected_at = Some(at + Duration::seconds(interval.min(86_400) as i64 + 30));
        entry.last_observed = Some(at);
        entry.gap = snapshot.windows.is_empty();
        let mut present = HashSet::new();
        for window in &snapshot.windows {
            if !valid_id(&window.id)
                || !window.used_percent.is_finite()
                || !(0.0..=100.0).contains(&window.used_percent)
                || window
                    .resets_at
                    .as_deref()
                    .is_some_and(|reset| timestamp(reset).is_none())
                || window.window_seconds == Some(0)
            {
                entry.gap = true;
                continue;
            }
            present.insert(window.id.clone());
            let observed_decrease = entry
                .last_percent
                .insert(window.id.clone(), window.used_percent)
                .is_some_and(|previous| window.used_percent < previous);
            let (earlier, latest) = latest_two(&entry.saved.points, &window.id);
            let mut gap_before = gap || !entry.present.contains(&window.id);
            if let Some(index) = latest {
                let previous = &entry.saved.points[index];
                let previous_at = timestamp(&previous.at).expect("validated history timestamp");
                if at <= previous_at {
                    entry.gap = true;
                    continue;
                }
                let boundary = previous.resets_at != window.resets_at
                    || previous.window_seconds != window.window_seconds
                    || window.used_percent < previous.used_percent;
                if observed_decrease && !boundary {
                    gap_before = true;
                }
                if !gap_before && !boundary && (at - previous_at).num_seconds() < 60 {
                    continue;
                }
                // An unchanged run stores only its first and latest reading.
                let unchanged =
                    !gap_before && !boundary && previous.used_percent == window.used_percent;
                let run_tail = earlier
                    .is_some_and(|earlier| continues_run(&entry.saved.points[earlier], previous));
                if unchanged && run_tail {
                    let mut tail = entry.saved.points.remove(index);
                    tail.at = snapshot.fetched_at.clone();
                    entry.saved.points.push(tail);
                    entry.dirty = true;
                    continue;
                }
            }
            entry.dirty = true;
            entry.saved.points.push(HistoryPoint {
                window_id: window.id.clone(),
                at: snapshot.fetched_at.clone(),
                used_percent: window.used_percent,
                resets_at: window.resets_at.clone(),
                window_seconds: window.window_seconds,
                gap_before,
            });
        }
        entry.last_percent.retain(|id, _| present.contains(id));
        entry.present = present;
        entry.dirty |= prune(&mut entry.saved.points, now);
        if entry.dirty {
            entry.error = save(&path, &entry.saved).err();
            entry.dirty = entry.error.is_some();
        }
    }

    /// Moves saved history to the account's new source when the identity still
    /// matches, so "Make active" doesn't read as a source change and wipe it.
    pub fn relocate(&mut self, account: &Account) {
        let path = self.directory.join(format!("{}.json", account.id));
        let entry = self.entry(account);
        let saved = &entry.saved.identity;
        let same_identity = match (&saved.history_key, &account.identity.history_key) {
            (Some(before), Some(now)) => before == now,
            (None, None) => saved.email.is_some() && saved.email == account.identity.email,
            _ => false,
        };
        if entry.unreadable
            || !same_identity
            || entry.saved.provider != account.provider
            || entry.saved.source == account.source
        {
            return;
        }
        entry.saved.source = account.source.clone();
        entry.gap = true;
        entry.error = save(&path, &entry.saved).err();
        entry.dirty = entry.error.is_some();
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        self.entries.remove(id);
        if !valid_id(id) {
            return Err(READ_ERROR.into());
        }
        match fs::remove_file(self.directory.join(format!("{id}.json"))) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("History could not be removed.".into()),
        }
    }
}

fn empty(account: &Account) -> SavedHistory {
    SavedHistory {
        version: 1,
        provider: account.provider,
        source: account.source.clone(),
        identity: account.identity.clone(),
        points: Vec::new(),
    }
}

fn reconcile(entry: &mut Entry, account: &Account) -> bool {
    let previous = &entry.saved.identity;
    let current = &account.identity;
    let same_identity = match (&previous.history_key, &current.history_key) {
        (Some(before), Some(now)) => before == now,
        (Some(_), None) => {
            entry.gap = true;
            return false;
        }
        (None, Some(_)) => false,
        (None, None) => {
            let Some(email) = &current.email else {
                entry.gap = true;
                return false;
            };
            previous.email.as_ref() == Some(email)
        }
    };
    if !same_identity
        || entry.saved.provider != account.provider
        || !same_source(&entry.saved.source, &account.source)
    {
        entry.saved = empty(account);
        entry.gap = true;
        entry.expected_at = None;
        entry.last_observed = None;
        entry.present.clear();
        entry.last_percent.clear();
        entry.dirty = true;
    } else {
        if previous.plan != current.plan {
            entry.gap = true;
        }
        if entry.saved.identity != *current || entry.saved.source != account.source {
            entry.saved.identity = current.clone();
            entry.saved.source = account.source.clone();
            entry.dirty = true;
        }
    }
    true
}

fn same_source(saved: &AccountSource, current: &AccountSource) -> bool {
    match (saved, current) {
        (
            AccountSource::ConfigDir { path: before, .. },
            AccountSource::ConfigDir { path: now, .. },
        ) => before == now,
        _ => saved == current,
    }
}

fn same_reading(a: &HistoryPoint, b: &HistoryPoint) -> bool {
    a.window_id == b.window_id
        && a.used_percent == b.used_percent
        && a.resets_at == b.resets_at
        && a.window_seconds == b.window_seconds
}

fn continues_run(earlier: &HistoryPoint, later: &HistoryPoint) -> bool {
    !later.gap_before && same_reading(earlier, later)
}

fn latest_two(points: &[HistoryPoint], window_id: &str) -> (Option<usize>, Option<usize>) {
    let mut found = points
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, point)| point.window_id == window_id)
        .map(|(index, _)| index);
    let latest = found.next();
    (found.next(), latest)
}

fn compact(points: &mut Vec<HistoryPoint>) -> bool {
    let mut middle = vec![false; points.len()];
    let mut runs: HashMap<&str, (usize, bool)> = HashMap::new();
    for (index, point) in points.iter().enumerate() {
        if let Some((previous, continued)) = runs.get(point.window_id.as_str()).copied() {
            let continues = continues_run(&points[previous], point);
            middle[previous] = continued && continues;
            runs.insert(&point.window_id, (index, continues));
        } else {
            runs.insert(&point.window_id, (index, false));
        }
    }
    let before = points.len();
    let mut index = 0;
    points.retain(|_| {
        index += 1;
        !middle[index - 1]
    });
    points.len() != before
}

fn prune(points: &mut Vec<HistoryPoint>, now: DateTime<Utc>) -> bool {
    let cutoff = now - Duration::days(7);
    let latest = now + Duration::seconds(30);
    let mut keep = vec![true; points.len()];
    let mut clamp = Vec::new();
    let mut later: HashMap<&str, (usize, DateTime<Utc>)> = HashMap::new();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let mut total = 0;
    for (index, point) in points.iter().enumerate().rev() {
        let window = point.window_id.as_str();
        let Some(at) = timestamp(&point.at).filter(|at| *at <= latest) else {
            keep[index] = false;
            continue;
        };
        let expired = at < cutoff;
        let held = |&(next, next_at): &(usize, DateTime<Utc>)| {
            next_at > cutoff && continues_run(point, &points[next])
        };
        let count = counts.entry(window).or_default();
        if (expired && !later.get(window).is_some_and(held))
            || *count >= MAX_WINDOW_POINTS
            || total >= MAX_POINTS
        {
            keep[index] = false;
            continue;
        }
        if expired {
            // The run was observed across the cutoff, so it starts there.
            clamp.push(index);
        }
        *count += 1;
        total += 1;
        later.insert(window, (index, at));
    }
    let changed = !clamp.is_empty() || keep.contains(&false);
    for index in clamp {
        points[index].at = cutoff.to_rfc3339();
    }
    let mut index = 0;
    points.retain(|_| {
        index += 1;
        keep[index - 1]
    });
    changed
}

fn timestamp(value: &str) -> Option<DateTime<Utc>> {
    (value.len() <= 64)
        .then(|| {
            DateTime::parse_from_rfc3339(value)
                .ok()
                .map(|at| at.with_timezone(&Utc))
        })
        .flatten()
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn validate(saved: &SavedHistory) -> Result<(), String> {
    if saved.version != 1 || saved.points.len() > MAX_POINTS {
        return Err(READ_ERROR.into());
    }
    for text in [
        &saved.identity.email,
        &saved.identity.plan,
        &saved.identity.history_key,
    ]
    .into_iter()
    .flatten()
    {
        if text.len() > 4096 || text.chars().any(char::is_control) {
            return Err(READ_ERROR.into());
        }
    }
    if !saved.source.is_valid_for(saved.provider) {
        return Err(READ_ERROR.into());
    }
    if saved
        .identity
        .history_key
        .as_ref()
        .is_some_and(|key| key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(READ_ERROR.into());
    }
    let mut latest = HashMap::new();
    for point in &saved.points {
        let at = timestamp(&point.at).ok_or(READ_ERROR)?;
        if !valid_id(&point.window_id)
            || !point.used_percent.is_finite()
            || !(0.0..=100.0).contains(&point.used_percent)
            || point
                .resets_at
                .as_deref()
                .is_some_and(|reset| timestamp(reset).is_none())
            || point.window_seconds == Some(0)
            || latest
                .insert(&point.window_id, at)
                .is_some_and(|last| last >= at)
        {
            return Err(READ_ERROR.into());
        }
    }
    Ok(())
}

fn load(path: &Path) -> Result<Option<SavedHistory>, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(READ_ERROR.into()),
    };
    let metadata = file.metadata().map_err(|_| READ_ERROR)?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err(READ_ERROR.into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| READ_ERROR)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(READ_ERROR.into());
    }
    let saved = serde_json::from_slice(&bytes).map_err(|_| READ_ERROR)?;
    validate(&saved)?;
    Ok(Some(saved))
}

fn save(path: &Path, saved: &SavedHistory) -> Result<(), String> {
    validate(saved)?;
    let parent = path.parent().ok_or(SAVE_ERROR)?;
    fs::create_dir_all(parent).map_err(|_| SAVE_ERROR)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| SAVE_ERROR)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| SAVE_ERROR)?;
    }
    let bytes = serde_json::to_vec(saved).map_err(|_| SAVE_ERROR)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(SAVE_ERROR.into());
    }
    file.write_all(&bytes).map_err(|_| SAVE_ERROR)?;
    file.flush().map_err(|_| SAVE_ERROR)?;
    file.as_file().sync_all().map_err(|_| SAVE_ERROR)?;
    file.persist(path).map_err(|_| SAVE_ERROR)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{Snapshot, UsageError, UsageWindow};

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_792_180_800 + seconds, 0).unwrap()
    }

    fn account(seconds: i64, used: f64) -> Account {
        Account {
            id: "codex-test".into(),
            provider: Provider::Codex,
            label: "Work".into(),
            color: "#ffffff".into(),
            source: AccountSource::ConfigDir {
                path: std::env::temp_dir().to_string_lossy().into_owned(),
                keychain_suffix: None,
            },
            identity: Identity {
                email: Some("example@example.test".into()),
                plan: Some("pro".into()),
                ..Identity::default()
            },
            snapshot: Some(Snapshot {
                fetched_at: at(seconds).to_rfc3339(),
                extra_usage: None,
                reset_grants: None,
                windows: vec![UsageWindow {
                    id: "session".into(),
                    label: "Session".into(),
                    used_percent: used,
                    resets_at: Some(at(18_000).to_rfc3339()),
                    window_seconds: Some(18_000),
                }],
            }),
            error: None,
            refreshing: false,
        }
    }

    #[test]
    fn persists_private_points_and_marks_restart_gap() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::new(directory.path().into());
        store.record(&account(0, 10.0), 60, at(0));
        store.record(&account(60, 20.0), 60, at(60));
        drop(store);
        let mut store = HistoryStore::new(directory.path().into());
        store.record(&account(120, 30.0), 60, at(120));
        let history = store.get(&account(120, 30.0), at(120));
        assert_eq!(history.points.len(), 3);
        assert!(history.error.is_none());
        assert!(!history.points[1].gap_before);
        assert!(history.points[2].gap_before);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(directory.path().join("codex-test.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let json = serde_json::to_value(history).unwrap();
        assert!(json.get("accountId").is_some());
        assert!(json.get("identity").is_none());
        assert!(json["points"][0].get("gapBefore").is_some());
    }

    #[test]
    fn corrupt_and_oversized_history_are_preserved_and_do_not_block_other_accounts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("codex-test.json");
        fs::write(&path, b"{damaged").unwrap();
        let mut store = HistoryStore::new(directory.path().into());
        store.record(&account(0, 10.0), 60, at(0));
        assert!(store.get(&account(0, 10.0), at(0)).error.is_some());
        assert_eq!(fs::read(&path).unwrap(), b"{damaged");
        let mut other = account(0, 10.0);
        other.id = "other".into();
        store.record(&other, 60, at(0));
        assert_eq!(store.get(&other, at(0)).points.len(), 1);
        store.remove("codex-test").unwrap();
        fs::File::create(&path)
            .unwrap()
            .set_len(MAX_BYTES + 1)
            .unwrap();
        assert!(store.get(&account(0, 10.0), at(0)).error.is_some());
        assert_eq!(fs::metadata(&path).unwrap().len(), MAX_BYTES + 1);
    }

    fn reading(window: &str, seconds: i64, used: f64) -> HistoryPoint {
        HistoryPoint {
            window_id: window.into(),
            at: at(seconds).to_rfc3339(),
            used_percent: used,
            resets_at: None,
            window_seconds: None,
            gap_before: false,
        }
    }

    fn changing(window: &str, seconds: std::ops::Range<i64>) -> Vec<HistoryPoint> {
        seconds
            .map(|second| reading(window, second, (second % 50) as f64))
            .collect()
    }

    #[test]
    fn caps_each_window_and_expires_points_without_inventing_readings() {
        let mut points = vec![reading("weekly", 0, 5.0)];
        points.extend(changing("session", 1..10_301));
        assert!(prune(&mut points, at(10_300)));
        assert_eq!(points.len(), MAX_WINDOW_POINTS + 1);
        assert_eq!(points[0].window_id, "weekly");
        assert_eq!(
            points[1].at,
            at(10_301 - MAX_WINDOW_POINTS as i64).to_rfc3339()
        );
        assert!(!prune(&mut points, at(10_300)));
        prune(
            &mut points,
            at(0) + Duration::days(7) + Duration::seconds(1),
        );
        assert_eq!(points.len(), MAX_WINDOW_POINTS);
        prune(&mut points, at(10_300) + Duration::days(8));
        assert!(points.is_empty());
        let mut future = vec![reading("session", 0, 5.0), reading("session", 60, 6.0)];
        assert!(prune(&mut future, at(0)));
        assert_eq!(future.len(), 1);
    }

    #[test]
    fn unchanged_run_crossing_the_age_limit_starts_at_the_limit() {
        let week = 7 * 24 * 60 * 60;
        let mut points = vec![
            reading("session", 0, 5.0),
            reading("session", 600, 5.0),
            reading("session", 1_200, 6.0),
        ];
        assert!(prune(&mut points, at(week + 300)));
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].at, at(300).to_rfc3339());
        assert_eq!(points[0].used_percent, 5.0);
        let mut gapped = vec![reading("session", 0, 5.0), reading("session", 600, 5.0)];
        gapped[1].gap_before = true;
        prune(&mut gapped, at(week + 300));
        assert_eq!(gapped.len(), 1);
        assert_eq!(gapped[0].at, at(600).to_rfc3339());
    }

    #[test]
    fn unchanged_readings_keep_the_first_and_latest_of_each_run() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::new(directory.path().into());
        for (seconds, used) in [(0, 10.0), (60, 10.0), (120, 10.0), (180, 10.0), (240, 12.0)] {
            store.record(&account(seconds, used), 60, at(seconds));
        }
        let mut reset = account(300, 12.0);
        reset.snapshot.as_mut().unwrap().windows[0].resets_at = Some(at(36_000).to_rfc3339());
        store.record(&reset, 60, at(300));
        for seconds in [360, 420] {
            reset.snapshot.as_mut().unwrap().fetched_at = at(seconds).to_rfc3339();
            store.record(&reset, 60, at(seconds));
        }
        store.failed(&reset);
        for seconds in [480, 540, 600] {
            reset.snapshot.as_mut().unwrap().fetched_at = at(seconds).to_rfc3339();
            store.record(&reset, 60, at(seconds));
        }
        let points = store.get(&reset, at(600)).points;
        let shape: Vec<_> = points
            .iter()
            .map(|point| (point.at.clone(), point.used_percent, point.gap_before))
            .collect();
        let expected: Vec<_> = [
            (0, 10.0, true),
            (180, 10.0, false),
            (240, 12.0, false),
            (300, 12.0, false),
            (420, 12.0, false),
            (480, 12.0, true),
            (600, 12.0, false),
        ]
        .into_iter()
        .map(|(seconds, used, gap)| (at(seconds).to_rfc3339(), used, gap))
        .collect();
        assert_eq!(shape, expected);
        assert_eq!(
            load(&directory.path().join("codex-test.json"))
                .unwrap()
                .unwrap()
                .points
                .len(),
            7
        );
    }

    #[test]
    fn skips_saving_when_nothing_changed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("codex-test.json");
        let mut store = HistoryStore::new(directory.path().into());
        store.record(&account(0, 10.0), 60, at(0));
        fs::remove_file(&path).unwrap();
        store.record(&account(30, 11.0), 60, at(30));
        let mut empty = account(40, 11.0);
        empty.snapshot.as_mut().unwrap().windows.clear();
        store.record(&empty, 60, at(40));
        assert!(!path.exists());
        store.record(&account(100, 11.0), 60, at(100));
        assert_eq!(load(&path).unwrap().unwrap().points.len(), 2);
    }

    #[test]
    fn dense_history_files_are_compacted_on_load() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("codex-test.json");
        let mut saved = empty(&account(0, 10.0));
        saved.points = [
            (0, 10.0),
            (60, 10.0),
            (120, 10.0),
            (180, 11.0),
            (240, 11.0),
            (300, 11.0),
        ]
        .into_iter()
        .map(|(seconds, used)| HistoryPoint {
            window_id: "session".into(),
            at: at(seconds).to_rfc3339(),
            used_percent: used,
            resets_at: Some(at(18_000).to_rfc3339()),
            window_seconds: Some(18_000),
            gap_before: seconds == 0,
        })
        .collect();
        saved.points[4].gap_before = true;
        let mut weekly = saved.points[1].clone();
        weekly.window_id = "weekly".into();
        saved.points.insert(2, weekly);
        save(&path, &saved).unwrap();
        let mut store = HistoryStore::new(directory.path().into());
        let kept: Vec<_> = store
            .get(&account(300, 11.0), at(300))
            .points
            .into_iter()
            .map(|point| (point.window_id, point.at))
            .collect();
        let expected: Vec<_> = [
            ("session", 0),
            ("weekly", 60),
            ("session", 120),
            ("session", 180),
            ("session", 240),
            ("session", 300),
        ]
        .into_iter()
        .map(|(window, seconds)| (window.to_string(), at(seconds).to_rfc3339()))
        .collect();
        assert_eq!(kept, expected);
    }

    #[test]
    fn linking_a_keychain_item_keeps_the_folder_history() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::new(directory.path().into());
        let mut folder = account(0, 10.0);
        folder.provider = Provider::Claude;
        store.record(&folder, 60, at(0));
        let path = match &folder.source {
            AccountSource::ConfigDir { path, .. } => path.clone(),
            AccountSource::Keychain => unreachable!(),
        };
        folder.source = AccountSource::ConfigDir {
            path: path.clone(),
            keychain_suffix: Some("0123abcd".into()),
        };
        folder.snapshot.as_mut().unwrap().fetched_at = at(60).to_rfc3339();
        store.record(&folder, 60, at(60));
        assert_eq!(store.get(&folder, at(60)).points.len(), 2);
        drop(store);
        let mut store = HistoryStore::new(directory.path().into());
        folder.source = AccountSource::ConfigDir {
            path,
            keychain_suffix: None,
        };
        assert_eq!(store.get(&folder, at(60)).points.len(), 2);
        folder.source = AccountSource::Keychain;
        assert!(store.get(&folder, at(60)).points.is_empty());
    }

    #[test]
    fn relocating_keeps_history_only_for_the_same_identity() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::new(directory.path().into());
        let mut person = account(0, 10.0);
        store.record(&person, 60, at(0));
        let moved = AccountSource::ConfigDir {
            path: directory
                .path()
                .join(".codex")
                .to_string_lossy()
                .into_owned(),
            keychain_suffix: None,
        };
        person.source = moved.clone();
        store.relocate(&person);
        assert_eq!(store.get(&person, at(0)).points.len(), 1);
        drop(store);
        let mut store = HistoryStore::new(directory.path().into());
        assert_eq!(store.get(&person, at(0)).points.len(), 1);

        let mut stranger = account(0, 10.0);
        stranger.id = "codex-other".into();
        store.record(&stranger, 60, at(0));
        stranger.source = moved;
        stranger.identity.email = Some("someone@example.test".into());
        store.relocate(&stranger);
        assert!(store.get(&stranger, at(0)).points.is_empty());
    }

    #[test]
    fn missing_stale_failed_and_out_of_order_samples_create_gaps_not_points() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::new(directory.path().into());
        store.record(&account(0, 10.0), 60, at(0));
        let mut stale = account(60, 20.0);
        stale.error = Some(UsageError::new("stale", "Cached"));
        store.record(&stale, 60, at(60));
        assert_eq!(store.get(&stale, at(60)).points.len(), 1);
        store.record(&account(120, 20.0), 60, at(120));
        store.failed(&account(120, 20.0));
        store.record(&account(180, 25.0), 60, at(180));
        store.record(&account(180, 25.0), 60, at(180));
        store.record(&account(170, 20.0), 60, at(180));
        let mut missing = account(240, 30.0);
        missing.snapshot = None;
        store.record(&missing, 60, at(240));
        store.record(&account(300, 35.0), 60, at(300));
        let history = store.get(&account(300, 35.0), at(300));
        assert_eq!(history.points.len(), 4);
        assert!(history.points.iter().all(|point| point.gap_before));
    }

    #[test]
    fn throttles_quick_refreshes_but_keeps_changed_reset_duration_and_decreases() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::new(directory.path().into());
        store.record(&account(0, 10.0), 60, at(0));
        store.record(&account(5, 11.0), 60, at(5));
        assert_eq!(store.get(&account(5, 11.0), at(5)).points.len(), 1);
        store.record(&account(10, 5.0), 60, at(10));
        let mut reset = account(15, 7.0);
        reset.snapshot.as_mut().unwrap().windows[0].resets_at = Some(at(36_000).to_rfc3339());
        store.record(&reset, 60, at(15));
        reset.snapshot.as_mut().unwrap().fetched_at = at(20).to_rfc3339();
        reset.snapshot.as_mut().unwrap().windows[0].window_seconds = Some(36_000);
        store.record(&reset, 60, at(20));
        let points = store.get(&reset, at(20)).points;
        assert_eq!(points.len(), 4);
        assert_eq!(points[1].used_percent, 5.0);
        assert!(!points[1].gap_before);
    }

    #[test]
    fn missing_window_breaks_only_its_returning_series() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::new(directory.path().into());
        let with_weekly = |seconds| {
            let mut value = account(seconds, 10.0);
            let mut weekly = value.snapshot.as_ref().unwrap().windows[0].clone();
            weekly.id = "weekly".into();
            value.snapshot.as_mut().unwrap().windows.push(weekly);
            value
        };
        store.record(&with_weekly(0), 60, at(0));
        store.record(&account(60, 20.0), 60, at(60));
        store.record(&with_weekly(120), 60, at(120));
        let history = store.get(&with_weekly(120), at(120));
        assert!(!history.points[3].gap_before);
        assert!(history.points[4].gap_before);
    }

    #[test]
    fn uses_previous_poll_expectation_across_interval_changes_and_sleep() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::new(directory.path().into());
        store.record(&account(0, 10.0), 600, at(0));
        store.record(&account(600, 20.0), 60, at(600));
        store.record(&account(660, 21.0), 600, at(660));
        store.record(&account(1_260, 22.0), 60, at(1_260));
        store.record(&account(1_860, 23.0), 60, at(1_860));
        let points = store.get(&account(1_860, 23.0), at(1_860)).points;
        assert!(!points[1].gap_before);
        assert!(!points[2].gap_before);
        assert!(!points[3].gap_before);
        assert!(points[4].gap_before);
    }

    #[test]
    fn identity_and_source_start_fresh_plan_marks_gap_and_removal_deletes_file() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::new(directory.path().into());
        store.record(&account(0, 10.0), 60, at(0));
        let mut changed = account(60, 20.0);
        changed.identity.email = Some("other@example.test".into());
        store.record(&changed, 60, at(60));
        assert_eq!(store.get(&changed, at(60)).points.len(), 1);
        changed.identity.plan = Some("plus".into());
        assert_eq!(store.get(&changed, at(60)).points.len(), 1);
        changed.snapshot.as_mut().unwrap().fetched_at = at(120).to_rfc3339();
        store.record(&changed, 60, at(120));
        assert!(store.get(&changed, at(120)).points[1].gap_before);
        changed.source = AccountSource::ConfigDir {
            path: "/other".into(),
            keychain_suffix: None,
        };
        assert!(store.get(&changed, at(120)).points.is_empty());
        store.remove(&changed.id).unwrap();
        assert!(!directory.path().join("codex-test.json").exists());
        assert!(store.get(&account(0, 10.0), at(120)).points.is_empty());
    }

    #[test]
    fn retries_failed_saves_without_blocking_readings_or_losing_the_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history");
        fs::write(&path, "blocked").unwrap();
        let mut store = HistoryStore::new(path.clone());
        fs::remove_file(&path).unwrap();
        assert!(store.get(&account(0, 10.0), at(0)).error.is_none());
        fs::write(&path, "blocked").unwrap();
        store.record(&account(0, 10.0), 60, at(0));
        let history = store.get(&account(0, 10.0), at(0));
        assert_eq!(history.points.len(), 1);
        assert_eq!(history.error.as_deref(), Some(SAVE_ERROR));
        fs::remove_file(&path).unwrap();
        store.record(&account(60, 20.0), 60, at(60));
        assert!(store.get(&account(60, 20.0), at(60)).error.is_none());
        let saved = load(&path.join("codex-test.json")).unwrap().unwrap();
        assert_eq!(saved.points.len(), 2);
        let json = fs::read_to_string(path.join("codex-test.json")).unwrap();
        assert!(!json.contains("accessToken"));
        assert!(!json.contains("refreshToken"));
        assert!(!json.contains("prompt"));
        assert!(!json.contains("extraUsage"));
    }

    #[test]
    fn missing_identity_pauses_history_without_erasing_verified_readings() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::new(directory.path().into());
        store.record(&account(0, 10.0), 60, at(0));
        let before = fs::read(directory.path().join("codex-test.json")).unwrap();
        let mut unknown = account(60, 20.0);
        unknown.identity.email = None;
        store.record(&unknown, 60, at(60));
        let history = store.get(&unknown, at(60));
        assert!(history.points.is_empty());
        assert_eq!(history.error.as_deref(), Some(IDENTITY_ERROR));
        assert_eq!(
            before,
            fs::read(directory.path().join("codex-test.json")).unwrap()
        );
        store.record(&account(120, 30.0), 60, at(120));
        let history = store.get(&account(120, 30.0), at(120));
        assert_eq!(history.points.len(), 2);
        assert!(history.points[1].gap_before);
        assert!(history.error.is_none());
    }

    #[test]
    fn stable_identity_separates_workspaces_and_missing_keys_cannot_append() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::new(directory.path().into());
        let mut first = account(0, 10.0);
        first.identity.history_key = Some("a".repeat(64));
        store.record(&first, 60, at(0));
        let mut unverified = account(60, 20.0);
        store.record(&unverified, 60, at(60));
        assert_eq!(
            store.get(&unverified, at(60)).error.as_deref(),
            Some(IDENTITY_ERROR)
        );
        unverified.identity.history_key = Some("a".repeat(64));
        unverified.identity.email = Some("renamed@example.test".into());
        store.record(&unverified, 60, at(60));
        assert_eq!(store.get(&unverified, at(60)).points.len(), 2);
        let mut second = account(120, 30.0);
        second.identity.history_key = Some("b".repeat(64));
        store.record(&second, 60, at(120));
        assert_eq!(store.get(&second, at(120)).points.len(), 1);
    }

    #[test]
    fn unidentified_first_reading_is_not_recorded() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::new(directory.path().into());
        let mut unknown = account(0, 10.0);
        unknown.identity.email = None;
        store.record(&unknown, 60, at(0));
        let history = store.get(&unknown, at(0));
        assert!(history.points.is_empty());
        assert_eq!(history.error.as_deref(), Some(IDENTITY_ERROR));
        assert!(!directory.path().join("codex-test.json").exists());
    }

    #[test]
    fn persisted_history_is_bounded_and_expired_readings_are_removed_on_save() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("codex-test.json");
        let mut saved = empty(&account(0, 10.0));
        saved.points = changing("session", 0..MAX_WINDOW_POINTS as i64);
        save(&path, &saved).unwrap();
        let mut store = HistoryStore::new(directory.path().into());
        let latest = MAX_WINDOW_POINTS as i64 + 60;
        store.record(&account(latest, 20.0), 60, at(latest));
        let persisted = load(&path).unwrap().unwrap();
        assert_eq!(persisted.points.len(), MAX_WINDOW_POINTS);
        assert_eq!(persisted.points[0].at, at(1).to_rfc3339());
        let later = latest + 8 * 24 * 60 * 60;
        store.record(&account(later, 25.0), 60, at(later));
        assert_eq!(load(&path).unwrap().unwrap().points.len(), 1);
    }

    #[test]
    fn decrease_after_a_throttled_increase_still_breaks_the_series() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = HistoryStore::new(directory.path().into());
        store.record(&account(0, 10.0), 60, at(0));
        store.record(&account(5, 20.0), 60, at(5));
        store.record(&account(10, 15.0), 60, at(10));
        let history = store.get(&account(10, 15.0), at(10));
        assert_eq!(history.points.len(), 2);
        assert_eq!(history.points[1].used_percent, 15.0);
        assert!(history.points[1].gap_before);
    }

    #[test]
    fn invalid_persisted_points_are_rejected() {
        let mut saved = empty(&account(0, 10.0));
        saved.points.push(HistoryPoint {
            window_id: "session".into(),
            at: at(0).to_rfc3339(),
            used_percent: 101.0,
            resets_at: None,
            window_seconds: None,
            gap_before: true,
        });
        assert!(validate(&saved).is_err());
        saved.points[0].used_percent = 10.0;
        saved.points.push(saved.points[0].clone());
        assert!(validate(&saved).is_err());
        saved.points.pop();
        saved.points[0].window_id = "../escape".into();
        assert!(validate(&saved).is_err());
    }
}
