use std::path::{Component, Path};

/// True when `executable` sits under `/Applications` or `<home>/Applications`.
/// A path with `..` counts as outside, since it cannot be compared without resolving it.
pub fn in_applications(executable: &Path, home: Option<&Path>) -> bool {
    if executable
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return false;
    }
    executable.starts_with("/Applications")
        || home.is_some_and(|home| {
            home.is_absolute() && executable.starts_with(home.join("Applications"))
        })
}

/// Start at login points at the running copy, so a copy on a DMG or in Downloads
/// stops launching once it is ejected or moved. An unknown path gives no warning.
pub fn warns_about_location(os: &str, executable: Option<&Path>, home: Option<&Path>) -> bool {
    os == "macos" && executable.is_some_and(|executable| !in_applications(executable, home))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    const HOME: &str = "/Users/alex";

    fn inside(path: &str) -> bool {
        in_applications(Path::new(path), Some(Path::new(HOME)))
    }

    #[test]
    fn applications_folders_count_as_installed() {
        assert!(inside("/Applications/Margin.app/Contents/MacOS/margin"));
        assert!(inside(
            "/Applications/Tools/Margin.app/Contents/MacOS/margin"
        ));
        assert!(inside(
            "/Users/alex/Applications/Margin.app/Contents/MacOS/margin"
        ));
    }

    #[test]
    fn other_locations_do_not() {
        for path in [
            "/Volumes/Margin/Margin.app/Contents/MacOS/margin",
            "/Users/alex/Downloads/Margin.app/Contents/MacOS/margin",
            "/Users/alex/Desktop/Margin.app/Contents/MacOS/margin",
            "/private/var/folders/ab/AppTranslocation/1/d/Margin.app/Contents/MacOS/margin",
            "/ApplicationsOld/Margin.app/Contents/MacOS/margin",
            "/Users/alex/ApplicationsOld/Margin.app/Contents/MacOS/margin",
            "/Users/sam/Applications/Margin.app/Contents/MacOS/margin",
            "/Applications/../Users/alex/Downloads/Margin.app/Contents/MacOS/margin",
            "Applications/Margin.app/Contents/MacOS/margin",
        ] {
            assert!(!inside(path), "{path}");
        }
    }

    #[test]
    fn a_missing_or_relative_home_only_allows_the_system_folder() {
        let personal = Path::new("/Users/alex/Applications/Margin.app/Contents/MacOS/margin");
        assert!(!in_applications(personal, None));
        assert!(!in_applications(
            Path::new("Applications/Margin.app"),
            Some(Path::new(""))
        ));
        assert!(in_applications(
            Path::new("/Applications/Margin.app/Contents/MacOS/margin"),
            None
        ));
    }

    #[test]
    fn warns_only_on_macos_outside_applications() {
        let downloads = Path::new("/Users/alex/Downloads/Margin.app/Contents/MacOS/margin");
        let installed = Path::new("/Applications/Margin.app/Contents/MacOS/margin");
        let home = Some(Path::new(HOME));
        assert!(warns_about_location("macos", Some(downloads), home));
        assert!(!warns_about_location("macos", Some(installed), home));
        assert!(!warns_about_location("macos", None, home));
        assert!(!warns_about_location("windows", Some(downloads), home));
        assert!(!warns_about_location(
            "windows",
            Some(Path::new(r"C:\Users\alex\Downloads\Margin.exe")),
            home
        ));
    }
}
