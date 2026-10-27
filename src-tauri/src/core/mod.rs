pub mod accounts;
pub mod burn;
pub mod digest;
pub mod history;
pub mod model;
pub mod notifications;
pub mod poller;
pub mod providers;
pub mod ring;
pub mod settings;
pub mod store;
pub mod switch;
pub mod tray;

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
