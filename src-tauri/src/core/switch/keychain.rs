//! Keychain items Make active reads and writes through `/usr/bin/security`.

/// `security -i` reads stdin lines into a 4096-byte buffer; longer lines are cut.
pub const STDIN_LINE_LIMIT: usize = 4096 - 64;

pub fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub use crate::core::hex;

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

/// The secret goes over stdin as hex, never argv, where other processes could
/// read it. `None` when the line would overflow `security -i`.
pub fn add_command(account: &str, service: &str, secret: &[u8]) -> Option<String> {
    let line = format!(
        "add-generic-password -U -a {} -s {} -X {}\n",
        quote(account),
        quote(service),
        hex(secret)
    );
    (line.len() <= STDIN_LINE_LIMIT).then_some(line)
}

/// Reads the `acct` attribute from `find-generic-password` output (no secret).
#[cfg(any(target_os = "macos", test))]
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

#[cfg(target_os = "macos")]
mod system {
    use super::{add_command, parse_account};
    use crate::core::security::{run, SecurityError};

    const LIMIT: u64 = 1_048_576;

    pub fn read(service: &str) -> Result<Option<Vec<u8>>, SecurityError> {
        run(&["find-generic-password", "-s", service, "-w"], None, LIMIT)?.password()
    }

    pub fn read_account(service: &str, account: &str) -> Result<Option<Vec<u8>>, SecurityError> {
        run(
            &["find-generic-password", "-a", account, "-s", service, "-w"],
            None,
            LIMIT,
        )?
        .password()
    }

    pub fn account(service: &str) -> Result<Option<String>, SecurityError> {
        let found = run(&["find-generic-password", "-s", service], None, LIMIT)?.found()?;
        Ok(found.and_then(|bytes| parse_account(&String::from_utf8_lossy(&bytes))))
    }

    pub fn write(service: &str, account: &str, secret: &[u8]) -> Result<(), SecurityError> {
        let line = add_command(account, service, secret).ok_or(SecurityError)?;
        run(&["-i"], Some(line.as_bytes()), LIMIT)?
            .succeeded()
            .map(|_| ())
    }

    pub fn remove(service: &str, account: &str) -> Result<(), SecurityError> {
        delete(service, account).map(|_| ())
    }

    /// Deletes the item without reading its secret; `false` when there was none.
    pub fn delete(service: &str, account: &str) -> Result<bool, SecurityError> {
        run(
            &["delete-generic-password", "-a", account, "-s", service],
            None,
            LIMIT,
        )?
        .found()
        .map(|found| found.is_some())
    }
}

#[cfg(target_os = "macos")]
pub use system::{account, delete, read, read_account, remove, write};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdin_line_quotes_the_service_and_hex_encodes_the_secret() {
        let secret = br#"{"claudeAiOauth":{"accessToken":"a\"b"}}"#;
        let line = add_command("ab\"c", "Claude Code-credentials-96e73013", secret)
            .expect("short secrets go over stdin");
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
    fn oversized_secrets_are_never_written() {
        let secret = vec![b'x'; STDIN_LINE_LIMIT / 2];
        assert_eq!(add_command("me", "Claude Code-credentials", &secret), None);
        let fits = vec![b'x'; 1500];
        assert!(add_command("me", "Claude Code-credentials", &fits).is_some());
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
