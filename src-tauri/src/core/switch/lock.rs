//! Claude Code's `proper-lockfile` protocol: a lock is a directory made with
//! `mkdir`. Margin waits a bounded time, never takes over a lock it did not make,
//! and keeps its own locks fresh while it holds them.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
pub enum LockError {
    Busy,
    Unavailable,
}

pub trait LockSet {
    fn acquire(&self, paths: &[PathBuf]) -> Result<Held, LockError>;
}

pub struct ProperLockfile {
    pub timeout: Duration,
}

impl Default for ProperLockfile {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(9),
        }
    }
}

/// Sorted and deduplicated, so two runs always lock in the same order.
pub fn lock_order(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths = paths.to_vec();
    paths.sort();
    paths.dedup();
    paths
}

impl LockSet for ProperLockfile {
    fn acquire(&self, paths: &[PathBuf]) -> Result<Held, LockError> {
        let deadline = Instant::now() + self.timeout;
        let mut held = Held::default();
        for (index, path) in lock_order(paths).into_iter().enumerate() {
            loop {
                match std::fs::create_dir(&path) {
                    Ok(()) => {
                        held.paths.push(path);
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        if Instant::now() >= deadline {
                            return Err(LockError::Busy);
                        }
                        let jitter = (index as u64 * 37 + held.paths.len() as u64 * 11) % 100;
                        std::thread::sleep(Duration::from_millis(120 + jitter));
                    }
                    Err(_) => return Err(LockError::Unavailable),
                }
            }
        }
        held.keep_fresh();
        Ok(held)
    }
}

#[derive(Default)]
pub struct Held {
    paths: Vec<PathBuf>,
    stop: Arc<AtomicBool>,
    toucher: Option<JoinHandle<()>>,
}

impl Held {
    #[cfg(test)]
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    fn keep_fresh(&mut self) {
        if self.paths.is_empty() {
            return;
        }
        let paths = self.paths.clone();
        let stop = self.stop.clone();
        self.toucher = Some(std::thread::spawn(move || {
            let mut last = Instant::now();
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(100));
                if last.elapsed() >= Duration::from_secs(3) {
                    last = Instant::now();
                    for path in &paths {
                        touch(path);
                    }
                }
            }
        }));
    }
}

fn touch(path: &std::path::Path) {
    #[cfg(unix)]
    if let Ok(directory) = std::fs::File::open(path) {
        let _ = directory.set_modified(std::time::SystemTime::now());
    }
    #[cfg(not(unix))]
    let _ = path;
}

impl Drop for Held {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(toucher) = self.toucher.take() {
            let _ = toucher.join();
        }
        for path in self.paths.iter().rev() {
            let _ = std::fs::remove_dir(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_are_taken_in_path_order_and_released_on_drop() {
        let directory = tempfile::tempdir().unwrap();
        let b = directory.path().join("b.lock");
        let a = directory.path().join("a.lock");
        let locks = ProperLockfile {
            timeout: Duration::from_millis(300),
        };
        let held = locks.acquire(&[b.clone(), a.clone(), b.clone()]).unwrap();
        assert_eq!(held.paths(), [a.clone(), b.clone()]);
        assert!(a.is_dir() && b.is_dir());
        drop(held);
        assert!(!a.exists() && !b.exists());
    }

    #[test]
    fn a_held_lock_times_out_releases_partial_locks_and_is_never_taken_over() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("a.lock");
        let busy = directory.path().join("b.lock");
        std::fs::create_dir(&busy).unwrap();
        let locks = ProperLockfile {
            timeout: Duration::from_millis(250),
        };
        let started = Instant::now();
        assert_eq!(
            locks.acquire(&[busy.clone(), first.clone()]).err(),
            Some(LockError::Busy)
        );
        assert!(started.elapsed() >= Duration::from_millis(250));
        assert!(busy.is_dir(), "someone else's lock stays in place");
        assert!(!first.exists(), "partial locks are released");
    }

    #[test]
    fn missing_parent_folder_is_an_error_not_a_wait() {
        let directory = tempfile::tempdir().unwrap();
        let locks = ProperLockfile {
            timeout: Duration::from_secs(5),
        };
        let started = Instant::now();
        assert_eq!(
            locks
                .acquire(&[directory.path().join("missing/x.lock")])
                .err(),
            Some(LockError::Unavailable)
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
