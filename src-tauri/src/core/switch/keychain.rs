//! macOS Keychain access through `/usr/bin/security`, the same binary Claude Code
//! uses, so item access lists keep trusting it and no prompt appears.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

/// `security -i` reads stdin lines into a 4096-byte buffer; longer lines are cut.
pub const STDIN_LINE_LIMIT: usize = 4096 - 64;

#[derive(Debug, PartialEq, Eq)]
pub enum AddCommand {
    Stdin(String),
    Argv(Vec<String>),
}

pub fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(text.get(index..index + 2)?, 16).ok())
        .collect()
}

/// Mirrors Claude Code: the secret goes over stdin as hex; argv only when the line
/// would overflow `security -i`.
pub fn add_command(account: &str, service: &str, secret: &[u8]) -> AddCommand {
    let encoded = hex(secret);
    let line = format!(
        "add-generic-password -U -a {} -s {} -X {encoded}\n",
        quote(account),
        quote(service)
    );
    if line.len() <= STDIN_LINE_LIMIT {
        AddCommand::Stdin(line)
    } else {
        AddCommand::Argv(
            [
                "add-generic-password",
                "-U",
                "-a",
                account,
                "-s",
                service,
                "-X",
                &encoded,
            ]
            .map(str::to_owned)
            .to_vec(),
        )
    }
}

/// Reads the `acct` attribute from `find-generic-password` output (no secret).
pub fn parse_account(attributes: &str) -> Option<String> {
    attributes.lines().find_map(|line| {
        let value = line
            .trim()
            .strip_prefix("\"acct\"<blob>=\"")?
            .strip_suffix('"')?;
        (!value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control))
            .then(|| value.to_owned())
    })
}

/// Claude Code's account name for new items: `$USER` when it is a plain name.
pub fn default_account() -> String {
    std::env::var("USER")
        .ok()
        .filter(|user| {
            !user.is_empty()
                && user
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        })
        .unwrap_or_else(|| "claude-code-user".into())
}

#[derive(Debug, PartialEq, Eq)]
pub struct KeychainError;

#[cfg(target_os = "macos")]
mod system {
    use super::{add_command, parse_account, AddCommand, KeychainError};
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const SECURITY: &str = "/usr/bin/security";
    const NOT_FOUND: i32 = 44;
    const LIMIT: u64 = 1_048_576;

    pub fn read(service: &str) -> Result<Option<Vec<u8>>, KeychainError> {
        password(&["find-generic-password", "-s", service, "-w"])
    }

    pub fn read_account(service: &str, account: &str) -> Result<Option<Vec<u8>>, KeychainError> {
        password(&["find-generic-password", "-a", account, "-s", service, "-w"])
    }

    fn password(args: &[&str]) -> Result<Option<Vec<u8>>, KeychainError> {
        let (status, mut bytes) = run(args, None)?;
        match status {
            0 => {
                if bytes.last() == Some(&b'\n') {
                    bytes.pop();
                }
                Ok(Some(bytes))
            }
            NOT_FOUND => Ok(None),
            _ => Err(KeychainError),
        }
    }

    pub fn account(service: &str) -> Result<Option<String>, KeychainError> {
        let (status, bytes) = run(&["find-generic-password", "-s", service], None)?;
        match status {
            0 => Ok(parse_account(&String::from_utf8_lossy(&bytes))),
            NOT_FOUND => Ok(None),
            _ => Err(KeychainError),
        }
    }

    pub fn write(service: &str, account: &str, secret: &[u8]) -> Result<(), KeychainError> {
        let (status, _) = match add_command(account, service, secret) {
            AddCommand::Stdin(line) => run(&["-i"], Some(line.into_bytes()))?,
            AddCommand::Argv(args) => {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                run(&args, None)?
            }
        };
        if status == 0 {
            Ok(())
        } else {
            Err(KeychainError)
        }
    }

    pub fn remove(service: &str, account: &str) -> Result<(), KeychainError> {
        let (status, _) = run(
            &["delete-generic-password", "-a", account, "-s", service],
            None,
        )?;
        if status == 0 || status == NOT_FOUND {
            Ok(())
        } else {
            Err(KeychainError)
        }
    }

    fn run(args: &[&str], input: Option<Vec<u8>>) -> Result<(i32, Vec<u8>), KeychainError> {
        let mut child = Command::new(SECURITY)
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| KeychainError)?;
        if let (Some(mut stdin), Some(input)) = (child.stdin.take(), input) {
            let written = stdin.write_all(&input);
            drop(stdin);
            if written.is_err() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(KeychainError);
            }
        }
        let stdout = child.stdout.take().ok_or(KeychainError)?;
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .take(LIMIT + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.code(),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        let bytes = reader
            .join()
            .map_err(|_| KeychainError)?
            .map_err(|_| KeychainError)?;
        match status {
            Some(code) if bytes.len() as u64 <= LIMIT => Ok((code, bytes)),
            _ => Err(KeychainError),
        }
    }
}

#[cfg(target_os = "macos")]
pub use system::{account, read, read_account, remove, write};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdin_line_quotes_the_service_and_hex_encodes_the_secret() {
        let secret = br#"{"claudeAiOauth":{"accessToken":"a\"b"}}"#;
        let AddCommand::Stdin(line) =
            add_command("ab\"c", "Claude Code-credentials-96e73013", secret)
        else {
            panic!("short secrets go over stdin");
        };
        assert_eq!(
            line,
            format!(
                "add-generic-password -U -a \"ab\\\"c\" -s \"Claude Code-credentials-96e73013\" -X {}\n",
                hex(secret)
            )
        );
        assert!(!line.contains("accessToken"));
        assert!(line.len() <= STDIN_LINE_LIMIT);
    }

    #[test]
    fn oversized_secrets_fall_back_to_argv() {
        let secret = vec![b'x'; STDIN_LINE_LIMIT / 2];
        let AddCommand::Argv(args) = add_command("me", "Claude Code-credentials", &secret) else {
            panic!("long secrets use argv");
        };
        assert_eq!(
            args[..6],
            [
                "add-generic-password",
                "-U",
                "-a",
                "me",
                "-s",
                "Claude Code-credentials"
            ]
        );
        assert_eq!(unhex(&args[7]).unwrap(), secret);
        let fits = vec![b'x'; 1500];
        assert!(matches!(
            add_command("me", "Claude Code-credentials", &fits),
            AddCommand::Stdin(_)
        ));
    }

    #[test]
    fn hex_round_trips_any_bytes() {
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(unhex(&hex(&bytes)).unwrap(), bytes);
        assert!(unhex("abc").is_none());
        assert!(unhex("zz").is_none());
    }

    #[test]
    fn account_attribute_is_parsed_without_the_secret() {
        let dump = "keychain: \"/Users/x/Library/Keychains/login.keychain-db\"\nattributes:\n    \"acct\"<blob>=\"abulkosim\"\n    \"svce\"<blob>=\"Claude Code-credentials\"\n";
        assert_eq!(parse_account(dump).as_deref(), Some("abulkosim"));
        assert_eq!(parse_account("    \"acct\"<blob>=<NULL>\n"), None);
        assert!(!default_account().is_empty());
    }
}
