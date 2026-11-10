#![no_main]

use libfuzzer_sys::fuzz_target;
use margin_lib::core::providers::{claude, codex};

fuzz_target!(|bytes: &[u8]| {
    if let Ok(value) = serde_json::from_slice(bytes) {
        let _ = claude::parse_usage(&value);
        let _ = codex::parse_usage(&value);
    }
});
