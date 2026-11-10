use crate::core::install::in_applications;
use std::path::Path;

pub const ARGS: [&str; 1] = ["--background"];

/// First `ProgramArguments` entry of the Launch Agent plist that `auto-launch` writes.
pub fn launch_agent_program(plist: &str) -> Option<&str> {
    let (_, rest) = plist.split_once("<key>ProgramArguments</key>")?;
    let rest = rest
        .trim_start()
        .strip_prefix("<array>")?
        .trim_start()
        .strip_prefix("<string>")?;
    rest.split_once("</string>").map(|(program, _)| program)
}

/// Program of a `Run` value that `auto-launch` writes: the path, a space, then the arguments.
pub fn run_value_program(value: &str) -> Option<&str> {
    value.strip_suffix(format!(" {}", ARGS.join(" ")).as_str())
}

/// Whether an enabled login item should be registered again for `current`.
/// On macOS only an installed copy takes it over, so a DMG, Downloads or translocated
/// copy never replaces a working entry. Windows has no install folder rule, so it only
/// replaces an entry whose program is gone.
pub fn needs_repair(
    os: &str,
    enabled: bool,
    current: &Path,
    registered: Option<&str>,
    registered_exists: bool,
    home: Option<&Path>,
) -> bool {
    if !enabled {
        return false;
    }
    let Some(path) = current.to_str() else {
        return false;
    };
    match os {
        "macos" => registered != Some(path) && in_applications(current, home),
        "windows" => {
            registered.is_some_and(|program| !program.eq_ignore_ascii_case(path))
                && !registered_exists
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
  <key>Label</key>
  <string>Margin</string>
  <key>AssociatedBundleIdentifiers</key>
  <array></array>
  <key>ProgramArguments</key>
  <array><string>/Users/alex/Downloads/Margin.app/Contents/MacOS/margin</string><string>--background</string></array>
  <key>RunAtLoad</key>
  <true/>
  </dict>
</plist>"#;

    #[test]
    fn reads_the_program_from_the_launch_agent() {
        assert_eq!(
            launch_agent_program(PLIST),
            Some("/Users/alex/Downloads/Margin.app/Contents/MacOS/margin")
        );
        let reformatted = "<key>ProgramArguments</key>\n\t<array>\n\t\t<string>/Applications/Margin.app/Contents/MacOS/margin</string>\n\t</array>";
        assert_eq!(
            launch_agent_program(reformatted),
            Some("/Applications/Margin.app/Contents/MacOS/margin")
        );
    }

    #[test]
    fn an_unreadable_launch_agent_has_no_program() {
        for plist in [
            "",
            "bplist00",
            "<key>ProgramArguments</key><array/>",
            "<key>ProgramArguments</key><array></array><string>/Applications/Margin.app</string>",
            "<key>ProgramArguments</key><array><string>/Applications/Margin.app",
        ] {
            assert_eq!(launch_agent_program(plist), None, "{plist}");
        }
    }

    #[test]
    fn reads_the_program_from_the_run_value() {
        assert_eq!(
            run_value_program(r"C:\Program Files\Margin\Margin.exe --background"),
            Some(r"C:\Program Files\Margin\Margin.exe")
        );
        assert_eq!(
            run_value_program(r"C:\Program Files\Margin\Margin.exe"),
            None
        );
        assert_eq!(run_value_program(r#""C:\Margin.exe" --hidden"#), None);
    }

    #[test]
    fn windows_replaces_only_a_missing_program() {
        let current = Path::new(r"C:\Users\alex\AppData\Local\Margin\Margin.exe");
        let old = Some(r"C:\Users\alex\Downloads\Margin.exe");
        let repair = |enabled, registered, exists| {
            needs_repair("windows", enabled, current, registered, exists, None)
        };
        assert!(repair(true, old, false));
        assert!(!repair(true, old, true));
        assert!(!repair(false, old, false));
        assert!(!repair(true, None, false));
        assert!(!repair(true, current.to_str(), true));
        assert!(!repair(
            true,
            Some(r"c:\users\alex\appdata\local\margin\margin.exe"),
            false
        ));
    }

    #[test]
    fn other_systems_never_repair() {
        let current = Path::new("/usr/bin/margin");
        assert!(!needs_repair("linux", true, current, None, false, None));
        assert!(!needs_repair(
            "linux",
            true,
            current,
            Some("/opt/margin"),
            false,
            None
        ));
    }

    #[cfg(unix)]
    #[test]
    fn macos_moves_the_entry_to_an_installed_copy_only() {
        let home = Some(Path::new("/Users/alex"));
        let installed = Path::new("/Applications/Margin.app/Contents/MacOS/margin");
        let personal = Path::new("/Users/alex/Applications/Margin.app/Contents/MacOS/margin");
        let downloads = "/Users/alex/Downloads/Margin.app/Contents/MacOS/margin";
        let repair = |enabled, current: &Path, registered, exists| {
            needs_repair("macos", enabled, current, registered, exists, home)
        };

        assert!(repair(true, installed, Some(downloads), false));
        assert!(repair(true, installed, Some(downloads), true));
        assert!(repair(true, personal, Some(downloads), false));
        assert!(repair(true, installed, None, false));
        assert!(!repair(true, installed, installed.to_str(), true));
        assert!(!repair(false, installed, Some(downloads), false));
        assert!(!repair(false, installed, None, false));

        for current in [
            downloads,
            "/Volumes/Margin/Margin.app/Contents/MacOS/margin",
            "/private/var/folders/ab/AppTranslocation/1/d/Margin.app/Contents/MacOS/margin",
            "/Users/alex/Desktop/Margin.app/Contents/MacOS/margin",
        ] {
            assert!(
                !repair(true, Path::new(current), installed.to_str(), true),
                "{current}"
            );
            assert!(!repair(true, Path::new(current), None, false), "{current}");
        }
    }
}
