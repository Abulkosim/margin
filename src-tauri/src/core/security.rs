//! Runs `/usr/bin/security`, the same binary Claude Code uses, so Keychain item
//! access lists keep trusting it and no prompt appears.

/// `security` exits with this status when the item does not exist.
const NOT_FOUND: i32 = 44;

#[derive(Debug, PartialEq, Eq)]
pub struct SecurityError;

/// Exit status and standard output of one `security` run.
#[derive(Debug)]
pub struct Output {
    pub status: i32,
    pub stdout: Vec<u8>,
}

impl Output {
    /// Standard output on success; any other status, including a missing item, is an error.
    pub fn succeeded(self) -> Result<Vec<u8>, SecurityError> {
        match self.status {
            0 => Ok(self.stdout),
            _ => Err(SecurityError),
        }
    }

    /// Standard output on success, `None` when the item does not exist.
    pub fn found(self) -> Result<Option<Vec<u8>>, SecurityError> {
        match self.status {
            0 => Ok(Some(self.stdout)),
            NOT_FOUND => Ok(None),
            _ => Err(SecurityError),
        }
    }

    /// A secret printed by `find-generic-password -w`, without the newline `security` adds.
    pub fn password(self) -> Result<Option<Vec<u8>>, SecurityError> {
        self.found().map(|found| {
            found.map(|mut bytes| {
                if bytes.last() == Some(&b'\n') {
                    bytes.pop();
                }
                bytes
            })
        })
    }
}

/// Runs `security` with `input` on stdin, killing it after five seconds. Fails
/// when it cannot start, is killed, or prints more than `limit` bytes.
#[cfg(target_os = "macos")]
pub fn run(args: &[&str], input: Option<&[u8]>, limit: u64) -> Result<Output, SecurityError> {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut child = Command::new("/usr/bin/security")
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| SecurityError)?;
    if let (Some(mut stdin), Some(input)) = (child.stdin.take(), input) {
        let written = stdin.write_all(input);
        drop(stdin);
        if written.is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SecurityError);
        }
    }
    let stdout = child.stdout.take().ok_or(SecurityError)?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let stdout = reader
        .join()
        .map_err(|_| SecurityError)?
        .map_err(|_| SecurityError)?;
    match status {
        Some(status) if stdout.len() as u64 <= limit => Ok(Output { status, stdout }),
        _ => Err(SecurityError),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(status: i32, stdout: &[u8]) -> Output {
        Output {
            status,
            stdout: stdout.to_vec(),
        }
    }

    #[test]
    fn only_a_zero_status_succeeds() {
        assert_eq!(output(0, b"{}\n").succeeded(), Ok(b"{}\n".to_vec()));
        assert_eq!(output(NOT_FOUND, b"").succeeded(), Err(SecurityError));
        assert_eq!(output(1, b"x").succeeded(), Err(SecurityError));
    }

    #[test]
    fn a_missing_item_is_not_an_error_when_looked_up() {
        assert_eq!(output(0, b"acct\n").found(), Ok(Some(b"acct\n".to_vec())));
        assert_eq!(output(NOT_FOUND, b"x").found(), Ok(None));
        assert_eq!(output(1, b"x").found(), Err(SecurityError));
        assert_eq!(output(-1, b"").found(), Err(SecurityError));
    }

    #[test]
    fn passwords_lose_only_the_trailing_newline() {
        assert_eq!(output(0, b"{}\n").password(), Ok(Some(b"{}".to_vec())));
        assert_eq!(output(0, b"{}\n\n").password(), Ok(Some(b"{}\n".to_vec())));
        assert_eq!(output(0, b"{}").password(), Ok(Some(b"{}".to_vec())));
        assert_eq!(output(0, b"").password(), Ok(Some(Vec::new())));
        assert_eq!(output(NOT_FOUND, b"\n").password(), Ok(None));
        assert_eq!(output(36, b"{}\n").password(), Err(SecurityError));
    }
}
