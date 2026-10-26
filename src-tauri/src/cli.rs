use crate::core::{
    model::{Account, AccountSource, Provider, UsageWindow, WindowScope},
    notifications::reset_clock,
    providers::claude::nfc,
    settings::{AccountOrder, Settings, TrayMode},
    store::{self, StoredState},
    tray,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use std::{
    ffi::{OsStr, OsString},
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};

pub const SCHEMA: u32 = 1;
pub const DATA_DIR_ENV: &str = "MARGIN_DATA_DIR";
const IDENTIFIER: &str = "com.abulkosim.margin";
const FLAGS: [&str; 10] = [
    "--json",
    "--line",
    "--all",
    "--short",
    "--color",
    "--no-color",
    "--help",
    "-h",
    "--version",
    "-V",
];
const USAGE: &str = "Usage: margin [--json | --line [--all] [--short] [--color | --no-color]]

  --json      Print every account as JSON
  --line      Print session and weekly usage for statuslines
  --all       With --line, print one row per account
  --short     With --line, print one account and one window
  --color     Force ANSI colors on (--no-color: off)
  --version   Print the version
  --help      Print this help

Reads the usage Margin last saved. Margin does not need to be running.
Colors are on in a terminal or under Claude Code, off when NO_COLOR is set.
The line is fitted to COLUMNS when set.";

const LABEL_MAX: usize = 12;
const LABEL_SHORT: usize = 4;
const CELL_WIDE: usize = 10;
const CELL_NARROW: usize = 4;
const DIVIDER: &str = " │ ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColorChoice {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LineOptions {
    all: bool,
    short: bool,
    color: ColorChoice,
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Json,
    Line(LineOptions),
    Help,
    Version,
}

/// What the caller's shell tells us: which CLI folders this terminal uses,
/// how wide it is and whether colors make sense.
struct Env {
    claude_dir: Option<OsString>,
    codex_dir: Option<OsString>,
    home: Option<PathBuf>,
    columns: Option<usize>,
    color: bool,
}

impl Env {
    fn capture(choice: ColorChoice) -> Self {
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        let color = color_enabled(
            choice,
            var("NO_COLOR").is_some(),
            io::stdout().is_terminal(),
            var("CLAUDECODE").is_some(),
        );
        Self {
            claude_dir: var("CLAUDE_CONFIG_DIR"),
            codex_dir: var("CODEX_HOME"),
            home: dirs::home_dir(),
            columns: var("COLUMNS")
                .and_then(|value| value.to_str()?.trim().parse::<usize>().ok())
                .filter(|columns| *columns > 0),
            color,
        }
    }

    fn plain() -> Self {
        Self {
            claude_dir: None,
            codex_dir: None,
            home: None,
            columns: None,
            color: false,
        }
    }
}

fn color_enabled(choice: ColorChoice, no_color: bool, tty: bool, claude_code: bool) -> bool {
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => !no_color && (tty || claude_code),
    }
}

pub fn run(args: impl IntoIterator<Item = OsString>) -> Option<i32> {
    let args: Vec<String> = args
        .into_iter()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let command = parse(&args)?;
    attach_console();
    let mut out = io::stdout().lock();
    let mut err = io::stderr().lock();
    let code = execute(command, &mut out, &mut err);
    let _ = out.flush();
    let _ = err.flush();
    Some(code)
}

fn parse(args: &[String]) -> Option<Result<Command, String>> {
    if !args.iter().any(|arg| FLAGS.contains(&arg.as_str())) {
        return None;
    }
    let (mut json, mut line, mut help, mut version) = (false, false, false, false);
    let (mut all, mut short, mut color, mut no_color) = (false, false, false, false);
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            "--line" => line = true,
            "--all" => all = true,
            "--short" => short = true,
            "--color" => color = true,
            "--no-color" => no_color = true,
            "--help" | "-h" => help = true,
            "--version" | "-V" => version = true,
            other => return Some(Err(format!("unknown option {other}"))),
        }
    }
    if help {
        return Some(Ok(Command::Help));
    }
    if version {
        return Some(Ok(Command::Version));
    }
    if json && line {
        return Some(Err("choose --json or --line, not both".into()));
    }
    if color && no_color {
        return Some(Err("choose --color or --no-color, not both".into()));
    }
    let line_only = [
        (all, "--all"),
        (short, "--short"),
        (color, "--color"),
        (no_color, "--no-color"),
    ];
    if let Some((_, flag)) = line_only.iter().find(|(set, _)| *set && !line) {
        return Some(Err(format!("{flag} works with --line")));
    }
    Some(if json {
        Ok(Command::Json)
    } else if line {
        Ok(Command::Line(LineOptions {
            all,
            short,
            color: match (color, no_color) {
                (true, _) => ColorChoice::Always,
                (_, true) => ColorChoice::Never,
                _ => ColorChoice::Auto,
            },
        }))
    } else {
        Err("choose --json or --line".into())
    })
}

fn execute(command: Result<Command, String>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let command = match command {
        Ok(command) => command,
        Err(message) => {
            let _ = writeln!(err, "margin: {message}\n\n{USAGE}");
            return 2;
        }
    };
    match command {
        Command::Help => {
            let _ = writeln!(out, "{USAGE}");
            0
        }
        Command::Version => {
            let _ = writeln!(out, "margin {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Command::Json | Command::Line(_) => {
            let state = match state_path()
                .ok_or_else(|| "could not find the app data folder".to_owned())
                .and_then(|path| read_state(&path))
            {
                Ok(state) => state,
                Err(message) => {
                    let _ = writeln!(err, "margin: {message}");
                    return 1;
                }
            };
            let env = match command {
                Command::Line(options) => Env::capture(options.color),
                _ => Env::plain(),
            };
            match render(&command, state.as_ref(), app_running, Utc::now(), &env) {
                Ok((text, empty)) => {
                    let _ = writeln!(out, "{text}");
                    i32::from(empty)
                }
                Err(message) => {
                    let _ = writeln!(err, "margin: {message}");
                    1
                }
            }
        }
    }
}

fn render(
    command: &Command,
    state: Option<&StoredState>,
    app_running: fn() -> Option<bool>,
    now: DateTime<Utc>,
    env: &Env,
) -> Result<(String, bool), String> {
    let empty = state.is_none_or(|state| state.accounts.is_empty());
    let text = match command {
        Command::Line(options) => {
            let default = Settings::default();
            let (accounts, settings) = state.map_or((&[][..], &default), |state| {
                (&state.accounts[..], &state.settings)
            });
            line(accounts, settings, *options, env, now)
        }
        _ => serde_json::to_string(&report(state, app_running(), now))
            .map_err(|_| "could not encode the report".to_owned())?,
    };
    Ok((text, empty))
}

fn state_path() -> Option<PathBuf> {
    std::env::var_os(DATA_DIR_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|dir| dir.join(IDENTIFIER)))
        .map(|dir| dir.join("state.json"))
}

fn read_state(path: &Path) -> Result<Option<StoredState>, String> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        _ => store::read(path).map(Some),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report<'a> {
    schema: u32,
    generated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    app_running: Option<bool>,
    accounts: Vec<AccountReport<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountReport<'a> {
    id: &'a str,
    provider: Provider,
    label: &'a str,
    email: Option<&'a str>,
    plan: Option<&'a str>,
    fetched_at: Option<&'a str>,
    stale: bool,
    status: Status,
    windows: Vec<WindowReport<'a>>,
    error: Option<ErrorReport<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum Status {
    Ok,
    Stale,
    SignedOut,
    Error,
    Pending,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowReport<'a> {
    id: &'a str,
    label: &'a str,
    scope: WindowScope,
    used_percent: f64,
    remaining_percent: f64,
    resets_at: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorReport<'a> {
    kind: &'a str,
    message: &'a str,
}

fn report(
    state: Option<&StoredState>,
    app_running: Option<bool>,
    now: DateTime<Utc>,
) -> Report<'_> {
    let accounts = state.map_or_else(Vec::new, |state| {
        ordered(&state.accounts, &state.settings, now)
            .into_iter()
            .map(|account| account_report(account, &state.settings, now))
            .collect()
    });
    Report {
        schema: SCHEMA,
        generated_at: now.to_rfc3339_opts(SecondsFormat::Secs, true),
        app_running,
        accounts,
    }
}

fn account_report<'a>(
    account: &'a Account,
    settings: &Settings,
    now: DateTime<Utc>,
) -> AccountReport<'a> {
    let stale = tray::stale(account, settings, now);
    AccountReport {
        id: &account.id,
        provider: account.provider,
        label: &account.label,
        email: account.identity.email.as_deref(),
        plan: account.identity.plan.as_deref(),
        fetched_at: account
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.fetched_at.as_str()),
        stale,
        status: status(account, stale),
        windows: account
            .snapshot
            .iter()
            .flat_map(|snapshot| &snapshot.windows)
            .map(|window| WindowReport {
                id: &window.id,
                label: &window.label,
                scope: window.scope(),
                used_percent: rounded(used(window)),
                remaining_percent: rounded(100.0 - used(window)),
                resets_at: window.resets_at.as_deref(),
            })
            .collect(),
        error: account.error.as_ref().map(|error| ErrorReport {
            kind: &error.kind,
            message: &error.message,
        }),
    }
}

fn status(account: &Account, stale: bool) -> Status {
    match &account.error {
        Some(error) if error.kind == "signedOut" => Status::SignedOut,
        Some(_) if account.snapshot.is_none() => Status::Error,
        _ if stale => Status::Stale,
        _ if account.snapshot.is_none() => Status::Pending,
        _ => Status::Ok,
    }
}

fn used(window: &UsageWindow) -> f64 {
    if window.used_percent.is_finite() {
        window.used_percent.clamp(0.0, 100.0)
    } else {
        100.0
    }
}

fn rounded(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn ordered<'a>(
    accounts: &'a [Account],
    settings: &Settings,
    now: DateTime<Utc>,
) -> Vec<&'a Account> {
    let mut accounts: Vec<&Account> = accounts.iter().collect();
    if settings.account_order == AccountOrder::Added {
        return accounts;
    }
    let pinned = (settings.tray_mode == TrayMode::Pinned)
        .then_some(settings.pinned_account_id.as_deref())
        .flatten();
    let rank = |account: &Account| -> (u8, f64) {
        if Some(account.id.as_str()) == pinned {
            return (0, 0.0);
        }
        let windows = account
            .snapshot
            .as_ref()
            .map_or(&[][..], |snapshot| &snapshot.windows);
        let core: Vec<&UsageWindow> = windows
            .iter()
            .filter(|window| window.scope() == WindowScope::Core)
            .collect();
        let session = core
            .iter()
            .position(|window| matches!(window.id.as_str(), "five_hour" | "primary_window"));
        let weekly = (0..core.len()).find(|index| Some(*index) != session);
        let lowest = [session, weekly]
            .into_iter()
            .flatten()
            .map(|index| 100.0 - used(core[index]))
            .reduce(f64::min);
        match lowest {
            Some(value) if !tray::stale(account, settings, now) => (1, value),
            _ => (2, 0.0),
        }
    };
    accounts.sort_by(|left, right| {
        let (left, right) = (rank(left), rank(right));
        left.0.cmp(&right.0).then(left.1.total_cmp(&right.1))
    });
    accounts
}

fn line(
    accounts: &[Account],
    settings: &Settings,
    options: LineOptions,
    env: &Env,
    now: DateTime<Utc>,
) -> String {
    if options.short {
        return short_line(accounts, settings, options.all, now);
    }
    if accounts.is_empty() {
        return "No accounts".into();
    }
    let rows: Vec<Vec<Segment>> = if options.all {
        let mut rows = Vec::new();
        for provider in [Provider::Claude, Provider::Codex] {
            let subset: Vec<Account> = accounts
                .iter()
                .filter(|account| account.provider == provider)
                .cloned()
                .collect();
            for account in ordered(&subset, settings, now) {
                rows.push(vec![segment(account, settings, now)]);
            }
        }
        rows
    } else {
        vec![[Provider::Claude, Provider::Codex]
            .into_iter()
            .filter_map(
                |provider| match chosen(accounts, provider, settings, env, now) {
                    Pick::Account(account) => Some(segment(account, settings, now)),
                    Pick::NotTracked => Some(Segment {
                        label: provider.name().to_owned(),
                        stale: false,
                        body: Body::Status("not tracked"),
                    }),
                    Pick::None => None,
                },
            )
            .collect()]
    };
    let rows = fit(&rows, options.all, env.columns);
    rows.iter()
        .map(|row| paint(row, env.color))
        .collect::<Vec<_>>()
        .join("\n")
}

enum Pick<'a> {
    Account(&'a Account),
    /// The terminal points at a folder Margin does not track; showing another
    /// account's usage here would be wrong.
    NotTracked,
    None,
}

/// The account this terminal is using, or the tray's pick for the provider
/// when no folder is configured.
fn chosen<'a>(
    accounts: &'a [Account],
    provider: Provider,
    settings: &Settings,
    env: &Env,
    now: DateTime<Utc>,
) -> Pick<'a> {
    let subset: Vec<&Account> = accounts
        .iter()
        .filter(|account| account.provider == provider)
        .collect();
    if subset.is_empty() {
        return Pick::None;
    }
    let (configured, folder) = match provider {
        Provider::Claude => (&env.claude_dir, ".claude"),
        Provider::Codex => (&env.codex_dir, ".codex"),
    };
    let home = env.home.as_deref();
    let default_dir = home.map(|home| normalize_dir(home.join(folder).as_os_str(), Some(home)));
    let target = configured
        .as_deref()
        .map(|value| normalize_dir(value, home))
        .or_else(|| default_dir.clone());
    if let Some(target) = target {
        let is_default = default_dir.as_ref() == Some(&target);
        let matched = subset.iter().find(|account| match &account.source {
            AccountSource::ConfigDir { path, .. } => {
                normalize_dir(OsStr::new(path), home) == target
            }
            AccountSource::Keychain => is_default,
        });
        if let Some(account) = matched {
            return Pick::Account(account);
        }
        if configured.is_some() {
            return Pick::NotTracked;
        }
    }
    let owned: Vec<Account> = subset.iter().map(|account| (*account).clone()).collect();
    let Some(pick) = tray::selected(&owned, settings, now) else {
        return Pick::None;
    };
    subset
        .into_iter()
        .find(|account| account.id == pick.id)
        .map_or(Pick::None, Pick::Account)
}

/// One comparable form for a CLI folder: `~` expanded, relative to the
/// working directory, symlinks resolved when it exists, no trailing slash,
/// NFC on macOS.
fn normalize_dir(value: &OsStr, home: Option<&Path>) -> String {
    let text = value.to_string_lossy();
    let text = text.trim();
    let mut path = PathBuf::from(text);
    if let Some(rest) = text.strip_prefix('~') {
        if rest.is_empty() || rest.starts_with(['/', '\\']) {
            if let Some(home) = home {
                path = home.join(rest.trim_start_matches(['/', '\\']));
            }
        }
    }
    if path.is_relative() {
        if let Ok(cwd) = std::env::current_dir() {
            path = cwd.join(path);
        }
    }
    let path = std::fs::canonicalize(&path).unwrap_or_else(|_| path.components().collect());
    nfc(&path.to_string_lossy())
}

enum Cell {
    Reading {
        remaining: f64,
        left: Option<String>,
    },
    Missing,
}

enum Body {
    Windows { session: Cell, weekly: Cell },
    Status(&'static str),
}

struct Segment {
    label: String,
    stale: bool,
    body: Body,
}

fn segment(account: &Account, settings: &Settings, now: DateTime<Utc>) -> Segment {
    let label = match account.label.trim() {
        "" => account.provider.name().to_owned(),
        label => label.to_owned(),
    };
    let stale = tray::stale(account, settings, now);
    let body = match (&account.snapshot, &account.error) {
        (_, Some(error)) if error.kind == "signedOut" => Body::Status("signed out"),
        (None, Some(_)) => Body::Status("no data"),
        (None, None) => Body::Status("…"),
        (Some(snapshot), _) => {
            let cell = |ids: [&str; 2]| {
                snapshot
                    .windows
                    .iter()
                    .find(|window| ids.contains(&window.id.as_str()))
                    .map_or(Cell::Missing, |window| Cell::Reading {
                        remaining: 100.0 - used(window),
                        left: window
                            .resets_at
                            .as_deref()
                            .and_then(|reset| time_left(reset, now)),
                    })
            };
            Body::Windows {
                session: cell(["five_hour", "primary_window"]),
                weekly: cell(["seven_day", "secondary_window"]),
            }
        }
    };
    Segment { label, stale, body }
}

/// Time until a reset in at most five characters; `None` once it has passed.
fn time_left(reset: &str, now: DateTime<Utc>) -> Option<String> {
    let reset = DateTime::parse_from_rfc3339(reset)
        .ok()?
        .with_timezone(&Utc);
    let seconds = (reset - now).num_seconds();
    if seconds <= 0 {
        return None;
    }
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    Some(if seconds < 60 {
        "<1m".into()
    } else if hours == 0 {
        format!("{minutes}m")
    } else if hours < 10 {
        format!("{hours}h{:02}m", minutes % 60)
    } else if days == 0 {
        format!("{hours}h")
    } else if hours % 24 == 0 {
        format!("{days}d")
    } else {
        format!("{days}d{}h", hours % 24)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Plain,
    Warn,
    Low,
    Out,
    Dim,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Span {
    text: String,
    tone: Tone,
}

impl Span {
    fn new(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            text: text.into(),
            tone,
        }
    }
}

fn tone_for(remaining: f64) -> Tone {
    if remaining <= 0.0 {
        Tone::Out
    } else if remaining < 20.0 {
        Tone::Low
    } else if remaining < 40.0 {
        Tone::Warn
    } else {
        Tone::Plain
    }
}

/// How much of the row survives a narrow terminal, widest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Detail {
    countdowns: bool,
    short_labels: bool,
    weekly: bool,
}

const DETAILS: [Detail; 4] = [
    Detail {
        countdowns: true,
        short_labels: false,
        weekly: true,
    },
    Detail {
        countdowns: false,
        short_labels: false,
        weekly: true,
    },
    Detail {
        countdowns: false,
        short_labels: true,
        weekly: true,
    },
    Detail {
        countdowns: false,
        short_labels: true,
        weekly: false,
    },
];

impl Detail {
    fn cell_width(self) -> usize {
        if self.countdowns {
            CELL_WIDE
        } else {
            CELL_NARROW
        }
    }

    fn gap(self) -> usize {
        if self.countdowns {
            2
        } else {
            1
        }
    }

    fn body_width(self) -> usize {
        if self.weekly {
            self.cell_width() * 2 + self.gap()
        } else {
            self.cell_width()
        }
    }

    fn label(self, label: &str) -> String {
        let limit = if self.short_labels {
            LABEL_SHORT
        } else {
            LABEL_MAX
        };
        if cells(label) <= limit {
            label.to_owned()
        } else if self.short_labels {
            clip(label, limit).to_owned()
        } else {
            let mut text = clip(label, limit - 1).to_owned();
            text.push('…');
            text
        }
    }
}

/// Terminal cells one character takes: East Asian wide forms two, combining
/// marks and zero-width joiners none, everything else one. A local
/// approximation that covers labels without a width table.
fn cell_width(ch: char) -> usize {
    let code = ch as u32;
    let combining = matches!(
        code,
        0x0300..=0x036F
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x200B..=0x200D
            | 0x20D0..=0x20FF
            | 0xFE00..=0xFE0F
            | 0xFE20..=0xFE2F
    );
    let wide = matches!(
        code,
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F900..=0x1F9FF
            | 0x20000..=0x3FFFD
    );
    if combining {
        0
    } else if wide {
        2
    } else {
        1
    }
}

fn cells(text: &str) -> usize {
    text.chars().map(cell_width).sum()
}

/// The longest prefix of `text` that fits `limit` cells.
fn clip(text: &str, limit: usize) -> &str {
    let mut used = 0;
    for (index, ch) in text.char_indices() {
        used += cell_width(ch);
        if used > limit {
            return &text[..index];
        }
    }
    text
}

fn width(spans: &[Span]) -> usize {
    spans.iter().map(|span| cells(&span.text)).sum()
}

/// A row never wider than `columns`: past the narrowest layout it is cut and
/// ends in an ellipsis.
fn bound(spans: &mut Vec<Span>, columns: usize) {
    if width(spans) <= columns {
        return;
    }
    let room = columns.saturating_sub(1);
    let mut kept: Vec<Span> = Vec::new();
    let mut used = 0;
    for span in spans.drain(..) {
        if used >= room {
            break;
        }
        let text = clip(&span.text, room - used).to_owned();
        used += cells(&text);
        kept.push(Span::new(text, span.tone));
    }
    while let Some(last) = kept.last_mut() {
        let trimmed = last.text.trim_end().len();
        if trimmed == 0 {
            kept.pop();
        } else {
            last.text.truncate(trimmed);
            break;
        }
    }
    kept.push(Span::new("…", Tone::Plain));
    *spans = kept;
}

fn pad(spans: &mut Vec<Span>, target: usize) {
    let current = width(spans);
    if current < target {
        spans.push(Span::new(" ".repeat(target - current), Tone::Plain));
    }
}

fn cell_spans(cell: &Cell, stale: bool, detail: Detail) -> Vec<Span> {
    let mut spans = match cell {
        Cell::Missing => vec![Span::new("–", Tone::Dim)],
        Cell::Reading { remaining, left } => {
            let tone = if stale {
                Tone::Dim
            } else {
                tone_for(*remaining)
            };
            let prefix = if stale { "~" } else { "" };
            let mut spans = vec![Span::new(
                format!("{prefix}{}%", tray::percent_label(*remaining)),
                tone,
            )];
            if let Some(left) = left.as_deref().filter(|_| detail.countdowns) {
                spans.push(Span::new(format!("·{left}"), Tone::Dim));
            }
            spans
        }
    };
    pad(&mut spans, detail.cell_width());
    spans
}

fn segment_spans(segment: &Segment, detail: Detail, label_width: Option<usize>) -> Vec<Span> {
    let label_tone = if segment.stale && matches!(segment.body, Body::Windows { .. }) {
        Tone::Dim
    } else {
        Tone::Plain
    };
    let mut spans = vec![Span::new(detail.label(&segment.label), label_tone)];
    if let Some(label_width) = label_width {
        pad(&mut spans, label_width);
    }
    spans.push(Span::new(" ", Tone::Plain));
    let mut body = match &segment.body {
        Body::Status(text) => vec![Span::new(
            *text,
            if *text == "…" {
                Tone::Dim
            } else {
                Tone::Plain
            },
        )],
        Body::Windows { session, weekly } => {
            let mut body = cell_spans(session, segment.stale, detail);
            if detail.weekly {
                body.push(Span::new(" ".repeat(detail.gap()), Tone::Plain));
                body.extend(cell_spans(weekly, segment.stale, detail));
            }
            body
        }
    };
    pad(&mut body, detail.body_width());
    spans.extend(body);
    spans
}

fn layout(rows: &[Vec<Segment>], detail: Detail, pad_labels: bool) -> Vec<Vec<Span>> {
    let label_width = pad_labels.then(|| {
        rows.iter()
            .flatten()
            .map(|segment| cells(&detail.label(&segment.label)))
            .max()
            .unwrap_or(0)
    });
    rows.iter()
        .map(|row| {
            let mut spans = Vec::new();
            for (index, segment) in row.iter().enumerate() {
                if index > 0 {
                    spans.push(Span::new(DIVIDER, Tone::Dim));
                }
                spans.extend(segment_spans(segment, detail, label_width));
            }
            while let Some(last) = spans.last_mut() {
                let trimmed = last.text.trim_end();
                if trimmed.is_empty() {
                    spans.pop();
                } else {
                    last.text.truncate(trimmed.len());
                    break;
                }
            }
            spans
        })
        .collect()
}

/// The widest layout whose longest row fits `columns`, else the narrowest.
fn fit(rows: &[Vec<Segment>], pad_labels: bool, columns: Option<usize>) -> Vec<Vec<Span>> {
    let mut laid_out = Vec::new();
    for detail in DETAILS {
        laid_out = layout(rows, detail, pad_labels);
        let widest = laid_out.iter().map(|row| width(row)).max().unwrap_or(0);
        if columns.is_none_or(|columns| widest <= columns) {
            break;
        }
    }
    if let Some(columns) = columns {
        for row in &mut laid_out {
            bound(row, columns);
        }
    }
    laid_out
}

fn paint(spans: &[Span], color: bool) -> String {
    let mut text = String::new();
    for span in spans {
        let code = match span.tone {
            Tone::Plain => None,
            Tone::Warn => Some("33"),
            Tone::Low => Some("31"),
            Tone::Out => Some("1;31"),
            Tone::Dim => Some("2"),
        };
        match code.filter(|_| color) {
            Some(code) => text.push_str(&format!("\x1b[{code}m{}\x1b[0m", span.text)),
            None => text.push_str(&span.text),
        }
    }
    text
}

fn short_line(accounts: &[Account], settings: &Settings, all: bool, now: DateTime<Utc>) -> String {
    if accounts.is_empty() {
        return "No accounts".into();
    }
    if all {
        return ordered(accounts, settings, now)
            .into_iter()
            .map(|account| short_segment(account, settings, false, now))
            .collect::<Vec<_>>()
            .join(" · ");
    }
    tray::selected(accounts, settings, now)
        .map(|account| short_segment(account, settings, true, now))
        .unwrap_or_else(|| "No accounts".into())
}

fn short_segment(
    account: &Account,
    settings: &Settings,
    with_reset: bool,
    now: DateTime<Utc>,
) -> String {
    let Some(window) = tray::limiting_window(account, settings.tray_metric, now) else {
        let state = match &account.error {
            Some(error) if error.kind == "signedOut" => "signed out",
            _ => "no data",
        };
        return format!("{} {state}", account.label);
    };
    let mut text = format!(
        "{} {}%",
        account.label,
        tray::percent_label(100.0 - used(window))
    );
    if with_reset {
        if let Some(reset) = window
            .resets_at
            .as_deref()
            .and_then(|reset| DateTime::parse_from_rfc3339(reset).ok())
            .map(|reset| reset.with_timezone(&Utc))
            .filter(|reset| *reset > now)
        {
            text.push_str(&format!(" · resets {}", reset_clock(reset, now)));
        }
    }
    if tray::stale(account, settings, now) {
        text.push_str(" (stale)");
    }
    text
}

#[cfg(target_os = "macos")]
fn app_running() -> Option<bool> {
    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::NSString;

    let current = NSRunningApplication::currentApplication();
    let running = NSRunningApplication::runningApplicationsWithBundleIdentifier(
        &NSString::from_str(IDENTIFIER),
    );
    Some(
        running
            .iter()
            .any(|app| !app.isTerminated() && *app != *current),
    )
}

#[cfg(windows)]
fn app_running() -> Option<bool> {
    use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;

    let wide = |text: String| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let class = wide(format!("{IDENTIFIER}-sic"));
    let title = wide(format!("{IDENTIFIER}-siw"));
    Some(!unsafe { FindWindowW(class.as_ptr(), title.as_ptr()) }.is_null())
}

#[cfg(not(any(target_os = "macos", windows)))]
fn app_running() -> Option<bool> {
    None
}

#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::{
        Foundation::{HANDLE, INVALID_HANDLE_VALUE},
        System::Console::{
            AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
            STD_OUTPUT_HANDLE,
        },
    };

    let usable = |handle: HANDLE| !handle.is_null() && handle != INVALID_HANDLE_VALUE;
    let handles = [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE].map(|id| (id, unsafe { GetStdHandle(id) }));
    if handles.iter().all(|(_, handle)| usable(*handle)) {
        return;
    }
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return;
    }
    for (id, handle) in handles {
        if usable(handle) {
            unsafe { SetStdHandle(id, handle) };
        }
    }
}

#[cfg(not(windows))]
fn attach_console() {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const NOW: &str = "2026-10-26T10:00:00Z";

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn at(hours: i64) -> String {
        (Utc::now() + chrono::Duration::hours(hours)).to_rfc3339_opts(SecondsFormat::Secs, true)
    }

    fn account(id: &str, label: &str, windows: Value) -> Value {
        json!({
            "id": id, "provider": "claude", "label": label, "color": "#ffffff",
            "source": { "kind": "configDir", "path": std::env::temp_dir().join("claude-secret-path") },
            "identity": { "email": format!("{id}@example.com"), "plan": "Max 5×" },
            "snapshot": { "windows": windows, "fetchedAt": at(0), "extraUsage": null },
            "error": null, "refreshing": false
        })
    }

    fn window(id: &str, used: f64, reset_hours: i64) -> Value {
        json!({ "id": id, "label": id, "usedPercent": used, "resetsAt": at(reset_hours), "windowSeconds": 18000 })
    }

    fn state(accounts: Vec<Value>, settings: Value) -> StoredState {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        let value = json!({ "version": 1, "accounts": accounts, "settings": settings });
        std::fs::write(&path, value.to_string()).unwrap();
        read_state(&path).unwrap().unwrap()
    }

    fn json_of(state: Option<&StoredState>, running: fn() -> Option<bool>) -> (Value, bool) {
        let (text, empty) =
            render(&Command::Json, state, running, Utc::now(), &Env::plain()).unwrap();
        (serde_json::from_str(&text).unwrap(), empty)
    }

    fn options(all: bool, short: bool) -> LineOptions {
        LineOptions {
            all,
            short,
            color: ColorChoice::Never,
        }
    }

    fn line_of(state: &StoredState, all: bool) -> String {
        render(
            &Command::Line(options(all, true)),
            Some(state),
            || None,
            Utc::now(),
            &Env::plain(),
        )
        .unwrap()
        .0
    }

    // Fixed-clock fixtures for the long line.

    fn now() -> DateTime<Utc> {
        NOW.parse().unwrap()
    }

    fn stamp(minutes: i64) -> String {
        (now() + chrono::Duration::minutes(minutes)).to_rfc3339_opts(SecondsFormat::Secs, true)
    }

    fn acct(provider: &str, id: &str, label: &str, windows: Value) -> Value {
        json!({
            "id": id, "provider": provider, "label": label, "color": "#ffffff",
            "source": { "kind": "configDir", "path": std::env::temp_dir().join(format!(".{provider}-{id}")) },
            "identity": { "email": null, "plan": null },
            "snapshot": { "windows": windows, "fetchedAt": stamp(0), "extraUsage": null },
            "error": null, "refreshing": false
        })
    }

    fn win(id: &str, used: f64, reset_minutes: i64) -> Value {
        json!({ "id": id, "label": id, "usedPercent": used, "resetsAt": stamp(reset_minutes), "windowSeconds": 18000 })
    }

    fn claude() -> Value {
        acct(
            "claude",
            "c",
            "Claude",
            json!([win("five_hour", 58.0, 138), win("seven_day", 14.0, 2880)]),
        )
    }

    fn claude_stale() -> Value {
        let mut claude = claude();
        claude["snapshot"]["fetchedAt"] = json!(stamp(-20));
        claude
    }

    fn codex() -> Value {
        acct(
            "codex",
            "x",
            "Codex",
            json!([
                win("primary_window", 5.0, 45),
                win("secondary_window", 40.0, 5760)
            ]),
        )
    }

    fn state_of(accounts: Vec<Value>) -> StoredState {
        state(accounts, json!({}))
    }

    fn long(state: &StoredState, all: bool, env: &Env) -> String {
        line(
            &state.accounts,
            &state.settings,
            options(all, false),
            env,
            now(),
        )
    }

    fn columns(columns: usize) -> Env {
        Env {
            columns: Some(columns),
            ..Env::plain()
        }
    }

    fn colored() -> Env {
        Env {
            color: true,
            ..Env::plain()
        }
    }

    fn strip(text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(start) = rest.find('\x1b') {
            out.push_str(&rest[..start]);
            let end = rest[start..].find('m').unwrap();
            rest = &rest[start + end + 1..];
        }
        out.push_str(rest);
        out
    }

    #[test]
    fn only_cli_flags_take_the_cli_path() {
        let line = |all, short, color| Some(Ok(Command::Line(LineOptions { all, short, color })));
        assert_eq!(parse(&args(&[])), None);
        assert_eq!(parse(&args(&["--background"])), None);
        assert_eq!(parse(&args(&["--json"])), Some(Ok(Command::Json)));
        assert_eq!(
            parse(&args(&["--line"])),
            line(false, false, ColorChoice::Auto)
        );
        assert_eq!(
            parse(&args(&["--all", "--line"])),
            line(true, false, ColorChoice::Auto)
        );
        assert_eq!(
            parse(&args(&["--line", "--short", "--all", "--no-color"])),
            line(true, true, ColorChoice::Never)
        );
        assert_eq!(
            parse(&args(&["--color", "--line"])),
            line(false, false, ColorChoice::Always)
        );
        assert_eq!(parse(&args(&["--json", "-h"])), Some(Ok(Command::Help)));
        assert_eq!(parse(&args(&["-V"])), Some(Ok(Command::Version)));
        for bad in [
            &["--json", "--line"][..],
            &["--all"],
            &["--short"],
            &["--color"],
            &["--json", "--all"],
            &["--json", "--short"],
            &["--json", "--no-color"],
            &["--json", "--background"],
            &["--line", "extra"],
            &["--line", "--color", "--no-color"],
        ] {
            assert!(matches!(parse(&args(bad)), Some(Err(_))), "{bad:?}");
        }
        assert_eq!(
            parse(&args(&["--short", "--all"])),
            Some(Err("--all works with --line".into()))
        );
    }

    #[test]
    fn help_version_and_bad_arguments_set_exit_codes() {
        let run = |values: &[&str]| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = execute(parse(&args(values)).unwrap(), &mut out, &mut err);
            (
                code,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        };
        let (code, out, _) = run(&["--help"]);
        assert_eq!(code, 0);
        assert!(out.starts_with("Usage: margin"));
        let (code, out, _) = run(&["--version"]);
        assert_eq!(code, 0);
        assert_eq!(out, format!("margin {}\n", env!("CARGO_PKG_VERSION")));
        let (code, out, err) = run(&["--json", "--nope"]);
        assert_eq!(code, 2);
        assert!(out.is_empty());
        assert!(err.contains("unknown option --nope"));
    }

    #[test]
    fn identifier_matches_the_app_config() {
        let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], IDENTIFIER);
    }

    #[test]
    fn json_reports_fresh_accounts_without_credentials() {
        let state = state(
            vec![account(
                "claude-a",
                "Personal",
                json!([
                    window("five_hour", 58.0, 2),
                    window("seven_day", 20.0, 48),
                    window("seven_day_opus", 90.5, 48),
                    window("seven_day_cowork", 10.0, 48)
                ]),
            )],
            json!({}),
        );
        let (value, empty) = json_of(Some(&state), || Some(true));
        assert!(!empty);
        assert_eq!(value["schema"], 1);
        assert_eq!(value["appRunning"], true);
        assert!(value["generatedAt"].as_str().unwrap().ends_with('Z'));
        let account = &value["accounts"][0];
        assert_eq!(account["id"], "claude-a");
        assert_eq!(account["provider"], "claude");
        assert_eq!(account["label"], "Personal");
        assert_eq!(account["email"], "claude-a@example.com");
        assert_eq!(account["plan"], "Max 5×");
        assert_eq!(account["status"], "ok");
        assert_eq!(account["stale"], false);
        assert!(account["error"].is_null());
        let windows = account["windows"].as_array().unwrap();
        assert_eq!(windows.len(), 4);
        assert_eq!(windows[0]["id"], "five_hour");
        assert_eq!(windows[0]["scope"], "core");
        assert_eq!(windows[0]["usedPercent"], 58.0);
        assert_eq!(windows[0]["remainingPercent"], 42.0);
        assert_eq!(windows[2]["scope"], "model");
        assert_eq!(windows[2]["remainingPercent"], 9.5);
        assert_eq!(windows[3]["scope"], "other");
        assert!(windows[0]["resetsAt"].as_str().is_some());
        let text = value.to_string();
        for secret in ["secret-path", "source", "historyKey", "color", "Token"] {
            assert!(!text.contains(secret), "{secret}");
        }
    }

    #[test]
    fn json_marks_stale_signed_out_and_failed_accounts() {
        let mut old = account("claude-old", "Old", json!([window("five_hour", 10.0, 2)]));
        old["snapshot"]["fetchedAt"] = json!(at(-2));
        let expired = account(
            "claude-exp",
            "Expired",
            json!([window("five_hour", 10.0, -1)]),
        );
        let mut retrying = account(
            "claude-retry",
            "Retry",
            json!([window("five_hour", 10.0, 2)]),
        );
        retrying["error"] =
            json!({ "kind": "rateLimited", "message": "Rate limited", "retryAfterSeconds": 60 });
        let mut signed_out = account("claude-out", "Out", json!([]));
        signed_out["snapshot"] = Value::Null;
        signed_out["error"] =
            json!({ "kind": "signedOut", "message": "Signed out", "retryAfterSeconds": null });
        let mut failed = account("claude-fail", "Fail", json!([]));
        failed["snapshot"] = Value::Null;
        failed["error"] =
            json!({ "kind": "network", "message": "Offline", "retryAfterSeconds": null });
        let mut pending = account("claude-new", "New", json!([]));
        pending["snapshot"] = Value::Null;
        let state = state(
            vec![old, expired, retrying, signed_out, failed, pending],
            json!({ "accountOrder": "added" }),
        );
        let (value, _) = json_of(Some(&state), || None);
        assert!(value.get("appRunning").is_none());
        let summary: Vec<(&str, &str, bool)> = value["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|account| {
                (
                    account["label"].as_str().unwrap(),
                    account["status"].as_str().unwrap(),
                    account["stale"].as_bool().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("Old", "stale", true),
                ("Expired", "stale", true),
                ("Retry", "ok", false),
                ("Out", "signedOut", true),
                ("Fail", "error", true),
                ("New", "pending", false),
            ]
        );
        assert_eq!(value["accounts"][3]["error"]["kind"], "signedOut");
        assert_eq!(value["accounts"][3]["fetchedAt"], Value::Null);
        assert_eq!(value["accounts"][3]["windows"], json!([]));
    }

    #[test]
    fn json_without_saved_state_is_empty() {
        let (value, empty) = json_of(None, || Some(false));
        assert!(empty);
        assert_eq!(value["accounts"], json!([]));
        assert_eq!(value["appRunning"], false);
        let state = state(Vec::new(), json!({}));
        assert!(json_of(Some(&state), || None).1);
    }

    #[test]
    fn unreadable_state_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        assert!(read_state(&path).unwrap().is_none());
        std::fs::write(&path, "{damaged").unwrap();
        assert!(read_state(&path).is_err());
        std::fs::write(&path, r#"{"version": 9}"#).unwrap();
        assert!(read_state(&path).is_err());
    }

    #[test]
    fn urgency_order_puts_pinned_then_lowest_then_stale() {
        let mut stale = account("c", "Stale", json!([window("five_hour", 99.0, 2)]));
        stale["snapshot"]["fetchedAt"] = json!(at(-3));
        let accounts = vec![
            account("a", "Roomy", json!([window("five_hour", 10.0, 2)])),
            stale,
            account(
                "b",
                "Tight",
                json!([window("five_hour", 10.0, 2), window("seven_day", 80.0, 48)]),
            ),
            account("d", "Pinned", json!([window("five_hour", 5.0, 2)])),
        ];
        let labels = |settings: Value| {
            let state = state(accounts.clone(), settings);
            ordered(&state.accounts, &state.settings, Utc::now())
                .iter()
                .map(|account| account.label.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(json!({})), ["Tight", "Roomy", "Pinned", "Stale"]);
        assert_eq!(
            labels(json!({ "trayMode": "pinned", "pinnedAccountId": "d" })),
            ["Pinned", "Tight", "Roomy", "Stale"]
        );
        assert_eq!(
            labels(json!({ "accountOrder": "added" })),
            ["Roomy", "Stale", "Tight", "Pinned"]
        );
    }

    #[test]
    fn short_line_follows_the_tray_account() {
        let accounts = vec![
            account(
                "a",
                "Personal",
                json!([
                    window("five_hour", 58.0, 2),
                    window("seven_day_opus", 99.0, 48)
                ]),
            ),
            account("b", "Work", json!([window("five_hour", 32.0, 2)])),
        ];
        let reset = |hours| {
            let reset = DateTime::parse_from_rfc3339(&at(hours))
                .unwrap()
                .with_timezone(&Utc);
            reset_clock(reset, Utc::now())
        };
        let lowest = state(accounts.clone(), json!({}));
        assert_eq!(
            line_of(&lowest, false),
            format!("Personal 42% · resets {}", reset(2))
        );
        let pinned = state(
            accounts.clone(),
            json!({ "trayMode": "pinned", "pinnedAccountId": "b" }),
        );
        assert_eq!(
            line_of(&pinned, false),
            format!("Work 68% · resets {}", reset(2))
        );
        let every = state(accounts.clone(), json!({ "trayMetric": "all" }));
        assert_eq!(
            line_of(&every, false),
            format!("Personal 1% · resets {}", reset(48))
        );
        assert_eq!(line_of(&lowest, true), "Personal 42% · Work 68%");
        let added = state(
            accounts,
            json!({ "accountOrder": "added", "trayMode": "pinned", "pinnedAccountId": "b" }),
        );
        assert_eq!(line_of(&added, true), "Personal 42% · Work 68%");
    }

    #[test]
    fn short_line_marks_stale_and_missing_readings() {
        let mut old = account("a", "Personal", json!([window("five_hour", 58.0, 2)]));
        old["snapshot"]["fetchedAt"] = json!(at(-2));
        let mut out = account("b", "Work", json!([]));
        out["snapshot"] = Value::Null;
        out["error"] =
            json!({ "kind": "signedOut", "message": "Signed out", "retryAfterSeconds": null });
        let mut new = account("c", "Spare", json!([]));
        new["snapshot"] = Value::Null;
        let only_old = state(vec![old.clone()], json!({}));
        assert!(line_of(&only_old, false).ends_with(" (stale)"));
        let mixed = state(vec![old, out, new], json!({ "accountOrder": "added" }));
        assert_eq!(
            line_of(&mixed, true),
            "Personal 42% (stale) · Work signed out · Spare no data"
        );
    }

    #[test]
    fn every_line_mode_reports_no_accounts() {
        let empty = state(Vec::new(), json!({}));
        for all in [false, true] {
            for short in [false, true] {
                for state in [Some(&empty), None] {
                    assert_eq!(
                        render(
                            &Command::Line(options(all, short)),
                            state,
                            || None,
                            Utc::now(),
                            &Env::plain()
                        )
                        .unwrap(),
                        ("No accounts".to_owned(), true)
                    );
                }
            }
        }
    }

    #[test]
    fn line_shows_both_providers_session_then_weekly() {
        let state = state_of(vec![codex(), claude()]);
        assert_eq!(
            long(&state, false, &Env::plain()),
            "Claude 42%·2h18m   86%·2d     │ Codex 95%·45m     60%·4d"
        );
    }

    #[test]
    fn line_shows_an_exhausted_session() {
        let exhausted = acct(
            "claude",
            "c",
            "Claude",
            json!([win("five_hour", 100.0, 67), win("seven_day", 39.0, 4320)]),
        );
        let state = state_of(vec![exhausted, codex()]);
        assert_eq!(
            long(&state, false, &Env::plain()),
            "Claude 0%·1h07m    61%·3d     │ Codex 95%·45m     60%·4d"
        );
    }

    #[test]
    fn line_marks_a_missing_window_with_a_dash() {
        let mut codex = codex();
        codex["snapshot"]["windows"] = json!([win("primary_window", 5.0, 45)]);
        let state = state_of(vec![claude(), codex]);
        assert_eq!(
            long(&state, false, &Env::plain()),
            "Claude 42%·2h18m   86%·2d     │ Codex 95%·45m     –"
        );
        let mut claude = claude();
        claude["snapshot"]["windows"] = json!([win("seven_day_opus", 50.0, 2880)]);
        let state = state_of(vec![claude]);
        assert_eq!(long(&state, false, &Env::plain()), "Claude –           –");
    }

    #[test]
    fn line_marks_stale_readings_with_a_tilde() {
        let state = state_of(vec![claude_stale(), codex()]);
        assert_eq!(
            long(&state, false, &Env::plain()),
            "Claude ~42%·2h18m  ~86%·2d    │ Codex 95%·45m     60%·4d"
        );
        let mut codex = codex();
        codex["error"] =
            json!({ "kind": "network", "message": "Offline", "retryAfterSeconds": null });
        let state = state_of(vec![claude_stale(), codex]);
        assert_eq!(
            long(&state, false, &Env::plain()),
            "Claude ~42%·2h18m  ~86%·2d    │ Codex ~95%·45m    ~60%·4d"
        );
        let mut passed = claude();
        passed["snapshot"]["windows"] = json!([win("five_hour", 58.0, -1)]);
        let state = state_of(vec![passed]);
        assert_eq!(long(&state, false, &Env::plain()), "Claude ~42%        –");
    }

    #[test]
    fn line_names_signed_out_pending_and_failed_accounts() {
        let mut codex = codex();
        codex["snapshot"] = Value::Null;
        codex["error"] =
            json!({ "kind": "signedOut", "message": "Signed out", "retryAfterSeconds": null });
        let state = state_of(vec![claude(), codex.clone()]);
        assert_eq!(
            long(&state, false, &Env::plain()),
            "Claude 42%·2h18m   86%·2d     │ Codex signed out"
        );
        codex["error"] = Value::Null;
        let state = state_of(vec![claude(), codex.clone()]);
        assert_eq!(
            long(&state, false, &Env::plain()),
            "Claude 42%·2h18m   86%·2d     │ Codex …"
        );
        codex["error"] =
            json!({ "kind": "network", "message": "Offline", "retryAfterSeconds": null });
        let state = state_of(vec![claude(), codex]);
        assert_eq!(
            long(&state, false, &Env::plain()),
            "Claude 42%·2h18m   86%·2d     │ Codex no data"
        );
    }

    #[test]
    fn line_with_one_provider_has_no_divider() {
        let state = state_of(vec![claude()]);
        assert_eq!(
            long(&state, false, &Env::plain()),
            "Claude 42%·2h18m   86%·2d"
        );
        let state = state_of(vec![codex()]);
        assert_eq!(
            long(&state, false, &Env::plain()),
            "Codex 95%·45m     60%·4d"
        );
    }

    #[test]
    fn line_degrades_step_by_step_to_fit_columns() {
        let state = state_of(vec![claude(), codex()]);
        let full = "Claude 42%·2h18m   86%·2d     │ Codex 95%·45m     60%·4d";
        assert_eq!(full.chars().count(), 56);
        assert_eq!(long(&state, false, &columns(56)), full);
        let no_countdowns = "Claude 42%  86%  │ Codex 95%  60%";
        assert_eq!(long(&state, false, &columns(55)), no_countdowns);
        assert_eq!(long(&state, false, &columns(40)), no_countdowns);
        assert_eq!(long(&state, false, &columns(33)), no_countdowns);
        let short_labels = "Clau 42%  86%  │ Code 95%  60%";
        assert_eq!(short_labels.chars().count(), 30);
        assert_eq!(long(&state, false, &columns(32)), short_labels);
        assert_eq!(long(&state, false, &columns(30)), short_labels);
        let session_only = "Clau 42%  │ Code 95%";
        assert_eq!(long(&state, false, &columns(29)), session_only);
        assert_eq!(long(&state, false, &columns(20)), session_only);
        assert_eq!(long(&state, false, &columns(19)), "Clau 42%  │ Code 9…");
        assert_eq!(long(&state, false, &columns(10)), "Clau 42%…");
        assert_eq!(long(&state, false, &columns(5)), "Clau…");
        assert_eq!(long(&state, false, &columns(1)), "…");
        let stale = state_of(vec![claude_stale(), codex()]);
        assert_eq!(
            long(&stale, false, &columns(40)),
            "Claude ~42% ~86% │ Codex 95%  60%"
        );
    }

    #[test]
    fn all_prints_one_padded_row_per_account() {
        let personal = acct(
            "claude",
            "p",
            "Personal",
            json!([win("five_hour", 58.0, 138), win("seven_day", 14.0, 2880)]),
        );
        let work = acct(
            "claude",
            "w",
            "Work",
            json!([win("five_hour", 29.0, 242), win("seven_day", 88.0, 1080)]),
        );
        let mut pending = codex();
        pending["snapshot"] = Value::Null;
        let state = state(
            vec![pending, personal, work],
            json!({ "accountOrder": "added" }),
        );
        assert_eq!(
            long(&state, true, &Env::plain()),
            "Personal 42%·2h18m   86%·2d\nWork     71%·4h02m   12%·18h\nCodex    …"
        );
        assert_eq!(
            long(&state, true, &columns(20)),
            "Personal 42%  86%\nWork     71%  12%\nCodex    …"
        );
        assert_eq!(
            long(&state, true, &columns(16)),
            "Pers 42%  86%\nWork 71%  12%\nCode …"
        );
        let urgency = state_of(vec![
            acct(
                "claude",
                "p",
                "Personal",
                json!([win("five_hour", 58.0, 138)]),
            ),
            acct("claude", "w", "Work", json!([win("five_hour", 90.0, 138)])),
        ]);
        assert_eq!(
            long(&urgency, true, &Env::plain()),
            "Work     10%·2h18m   –\nPersonal 42%·2h18m   –"
        );
    }

    #[test]
    fn labels_are_truncated_by_characters() {
        let state = state_of(vec![acct(
            "claude",
            "c",
            "Ünïcödé Workspace Account",
            json!([win("five_hour", 58.0, 138)]),
        )]);
        assert_eq!(
            long(&state, false, &Env::plain()),
            "Ünïcödé Wor… 42%·2h18m   –"
        );
        assert_eq!(long(&state, false, &columns(18)), "Ünïc 42%  –");
    }

    #[test]
    fn wide_and_combining_characters_are_measured_in_cells() {
        assert_eq!(cells("Ünïc"), 4);
        assert_eq!(cells("e\u{301}"), 1, "a combining accent takes no cell");
        assert_eq!(cells("日本語"), 6);
        assert_eq!(cells("a\u{200d}b"), 2, "a zero-width joiner takes no cell");
        assert_eq!(clip("日本語", 5), "日本");
        assert_eq!(clip("日本語", 6), "日本語");
        let state = state_of(vec![acct(
            "claude",
            "c",
            "日本語アカウント",
            json!([win("five_hour", 58.0, 138)]),
        )]);
        assert_eq!(
            long(&state, false, &Env::plain()),
            "日本語アカ… 42%·2h18m   –",
            "eleven cells of label, then the ellipsis"
        );
        assert_eq!(long(&state, false, &columns(18)), "日本語アカ… 42%  –");
        assert_eq!(long(&state, false, &columns(17)), "日本 42%  –");
        assert_eq!(long(&state, false, &columns(7)), "日本 4…");
        assert_eq!(long(&state, false, &columns(3)), "日…");
        let both = state_of(vec![
            acct(
                "claude",
                "c",
                "日本語",
                json!([win("five_hour", 58.0, 138)]),
            ),
            acct("claude", "d", "Work", json!([win("five_hour", 58.0, 138)])),
        ]);
        assert_eq!(
            long(&both, true, &columns(14)),
            "日本語 42%  –\nWork   42%  –",
            "labels pad to cells"
        );
        assert_eq!(
            long(&both, true, &columns(7)),
            "日本 4…\nWork 4…",
            "every row is bounded"
        );
    }

    #[test]
    fn colors_follow_remaining_and_dim_the_rest() {
        let claude = acct(
            "claude",
            "c",
            "Claude",
            json!([win("five_hour", 70.0, 138), win("seven_day", 90.0, 2880)]),
        );
        let codex = acct(
            "codex",
            "x",
            "Codex",
            json!([
                win("primary_window", 100.0, 45),
                win("secondary_window", 40.0, 5760)
            ]),
        );
        let state = state_of(vec![claude, codex]);
        let painted = long(&state, false, &colored());
        let plain = long(&state, false, &Env::plain());
        assert_eq!(strip(&painted), plain);
        assert!(!plain.contains('\x1b'));
        assert_eq!(
            painted,
            "Claude \x1b[33m30%\x1b[0m\x1b[2m·2h18m\x1b[0m   \x1b[31m10%\x1b[0m\x1b[2m·2d\x1b[0m    \
             \x1b[2m │ \x1b[0mCodex \x1b[1;31m0%\x1b[0m\x1b[2m·45m\x1b[0m      60%\x1b[2m·4d\x1b[0m"
        );
        let stale = long(&state_of(vec![claude_stale()]), false, &colored());
        assert_eq!(
            stale,
            "\x1b[2mClaude\x1b[0m \x1b[2m~42%\x1b[0m\x1b[2m·2h18m\x1b[0m  \
             \x1b[2m~86%\x1b[0m\x1b[2m·2d\x1b[0m"
        );
    }

    #[test]
    fn color_defaults_to_terminals_and_claude_code_unless_no_color() {
        use ColorChoice::*;
        assert!(color_enabled(Auto, false, true, false));
        assert!(color_enabled(Auto, false, false, true));
        assert!(!color_enabled(Auto, false, false, false));
        assert!(!color_enabled(Auto, true, true, true));
        assert!(color_enabled(Always, true, false, false));
        assert!(!color_enabled(Never, false, true, true));
    }

    #[test]
    fn time_left_fits_five_characters() {
        let left = |seconds: i64| {
            time_left(
                &(now() + chrono::Duration::seconds(seconds)).to_rfc3339(),
                now(),
            )
        };
        assert_eq!(left(-1), None);
        assert_eq!(left(0), None);
        assert_eq!(left(30).as_deref(), Some("<1m"));
        assert_eq!(left(59 * 60 + 59).as_deref(), Some("59m"));
        assert_eq!(left(3600).as_deref(), Some("1h00m"));
        assert_eq!(left(2 * 3600 + 5 * 60).as_deref(), Some("2h05m"));
        assert_eq!(left(9 * 3600 + 59 * 60 + 59).as_deref(), Some("9h59m"));
        assert_eq!(left(10 * 3600).as_deref(), Some("10h"));
        assert_eq!(left(23 * 3600 + 59 * 60).as_deref(), Some("23h"));
        assert_eq!(left(24 * 3600).as_deref(), Some("1d"));
        assert_eq!(left(25 * 3600).as_deref(), Some("1d1h"));
        assert_eq!(
            left(6 * 86_400 + 23 * 3600 + 59 * 60).as_deref(),
            Some("6d23h")
        );
        assert_eq!(time_left("soon", now()), None);
    }

    fn env_home(home: &Path, claude: Option<&str>, codex: Option<&str>) -> Env {
        Env {
            claude_dir: claude.map(OsString::from),
            codex_dir: codex.map(OsString::from),
            home: Some(home.to_path_buf()),
            ..Env::plain()
        }
    }

    fn at_path(provider: &str, id: &str, label: &str, used: f64, path: &Path) -> Value {
        let session = if provider == "claude" {
            "five_hour"
        } else {
            "primary_window"
        };
        let mut value = acct(provider, id, label, json!([win(session, used, 138)]));
        value["source"] = json!({ "kind": "configDir", "path": path });
        value
    }

    #[test]
    fn line_picks_the_account_of_the_callers_config_dir() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        for folder in [".claude", ".claude-work", ".codex", ".codex-work"] {
            std::fs::create_dir(home.join(folder)).unwrap();
        }
        let accounts = vec![
            at_path("claude", "a", "Personal", 58.0, &home.join(".claude")),
            at_path("claude", "b", "Work", 90.0, &home.join(".claude-work")),
            at_path("codex", "x", "Codex", 5.0, &home.join(".codex")),
            at_path("codex", "y", "Codex Work", 50.0, &home.join(".codex-work")),
        ];
        let fresh = state_of(accounts.clone());
        let pick = |claude: Option<&str>, codex: Option<&str>| {
            long(&fresh, false, &env_home(home, claude, codex))
        };
        assert_eq!(
            pick(None, None),
            "Personal 42%·2h18m   –          │ Codex 95%·2h18m   –"
        );
        let work = home.join(".claude-work");
        let work = work.to_str().unwrap();
        assert!(pick(Some(work), None).starts_with("Work 10%"));
        assert!(pick(Some(&format!("{work}/")), None).starts_with("Work 10%"));
        assert!(pick(Some("~/.claude-work"), None).starts_with("Work 10%"));
        assert!(pick(Some(&format!("  {work}  ")), None).starts_with("Work 10%"));
        assert!(pick(Some("~/.claude/"), None).starts_with("Personal 42%"));
        assert!(pick(None, Some("~/.codex-work")).ends_with("│ Codex Work 50%·2h18m   –"));
        let unknown = pick(Some("/nowhere/.claude"), None);
        assert!(unknown.starts_with("Claude not tracked"), "{unknown}");
        assert!(unknown.ends_with("│ Codex 95%·2h18m   –"), "{unknown}");
        let pinned = state(
            accounts,
            json!({ "trayMode": "pinned", "pinnedAccountId": "a" }),
        );
        assert_eq!(
            long(&pinned, false, &env_home(home, None, None)),
            "Personal 42%·2h18m   –          │ Codex 95%·2h18m   –"
        );
        let tray_pick = long(&pinned, false, &Env::plain());
        assert!(
            tray_pick.starts_with("Personal 42%"),
            "no folder at all: the tray's pick, {tray_pick}"
        );
        let untracked = long(
            &pinned,
            false,
            &env_home(home, Some("/nowhere/.claude"), Some("/nowhere/.codex")),
        );
        assert_eq!(
            untracked,
            "Claude not tracked            │ Codex not tracked"
        );
    }

    #[test]
    fn keychain_account_is_the_default_claude_folder() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        for folder in [".claude", ".claude-work"] {
            std::fs::create_dir(home.join(folder)).unwrap();
        }
        let mut keychain = acct(
            "claude",
            "k",
            "Keychain",
            json!([win("five_hour", 58.0, 138)]),
        );
        keychain["source"] = json!({ "kind": "keychain" });
        let state = state_of(vec![
            at_path("claude", "b", "Work", 90.0, &home.join(".claude-work")),
            keychain,
        ]);
        let pick = |claude: Option<&str>| long(&state, false, &env_home(home, claude, None));
        assert!(pick(None).starts_with("Keychain 42%"));
        assert!(pick(Some("~/.claude")).starts_with("Keychain 42%"));
        assert!(pick(home.join(".claude/").to_str()).starts_with("Keychain 42%"));
        assert!(pick(Some("~/.claude-work")).starts_with("Work 10%"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn config_dir_match_ignores_unicode_normalization() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let composed = home.join(".claude-caf\u{e9}");
        std::fs::create_dir(&composed).unwrap();
        std::fs::create_dir(home.join(".claude")).unwrap();
        let state = state_of(vec![
            at_path("claude", "a", "Personal", 58.0, &home.join(".claude")),
            at_path("claude", "b", "Caf\u{e9}", 90.0, &composed),
        ]);
        let decomposed = home.join(".claude-cafe\u{301}");
        let picked = long(&state, false, &env_home(home, decomposed.to_str(), None));
        assert!(picked.starts_with("Caf\u{e9} 10%"), "{picked}");
    }

    #[test]
    fn normalize_dir_expands_home_and_strips_slashes() {
        let home = Path::new("/Users/example");
        let normalized = |value: &str| normalize_dir(OsStr::new(value), Some(home));
        assert_eq!(normalized("~"), normalized("/Users/example"));
        assert_eq!(
            normalized("~/.claude/"),
            normalized("/Users/example/.claude")
        );
        assert_eq!(normalized("/opt/claude//"), normalized("/opt/claude"));
        assert_eq!(normalized("/opt/./claude/"), normalized("/opt/claude"));
        assert!(normalized("~/.claude/").ends_with(".claude"));
        assert_eq!(
            normalized("~user/.claude"),
            normalize_dir(OsStr::new("~user/.claude"), None)
        );
    }
}
