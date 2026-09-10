use mega_win_alt_tab::core::{AppEntry, AppSource};
use std::fs;
use std::path::{Path, PathBuf};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub(super) fn enumerate_apps() -> Vec<AppEntry> {
    let mut apps = Vec::new();
    for (root, source) in app_search_roots() {
        collect_apps_from_dir(&root, source, &mut apps);
    }
    apps
}

fn app_search_roots() -> Vec<(PathBuf, AppSource)> {
    let mut roots = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push((
            PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
            AppSource::UserStartMenu,
        ));
    }
    if let Some(program_data) = std::env::var_os("ProgramData") {
        roots.push((
            PathBuf::from(program_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
            AppSource::AllUsersStartMenu,
        ));
    }
    roots
}

fn collect_apps_from_dir(root: &Path, source: AppSource, apps: &mut Vec<AppEntry>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_apps_from_dir(&path, source, apps);
            continue;
        }

        if !is_launchable_shortcut(&path) {
            continue;
        }

        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };

        apps.push(AppEntry {
            name: name.to_string(),
            launch_path: path.to_string_lossy().to_string(),
            source,
        });
    }
}

fn is_launchable_shortcut(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("lnk")
                || extension.eq_ignore_ascii_case("appref-ms")
                || extension.eq_ignore_ascii_case("url")
        })
}

pub(super) unsafe fn launch_app(hwnd: HWND, launch_path: &str) -> bool {
    let file = to_wide_z(launch_path);
    let result = ShellExecuteW(
        hwnd,
        w!("open"),
        PCWSTR(file.as_ptr()),
        PCWSTR::null(),
        PCWSTR::null(),
        SW_SHOWNORMAL,
    );
    result.0 as isize > 32
}

fn to_wide_z(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "mega-win-alt-tab-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("test directory should be created");
        dir
    }

    #[test]
    fn launchable_shortcut_filter_accepts_start_menu_file_types() {
        assert!(is_launchable_shortcut(Path::new("Signal.lnk")));
        assert!(is_launchable_shortcut(Path::new("ClickOnce.APPREF-MS")));
        assert!(is_launchable_shortcut(Path::new("Website.URL")));
        assert!(!is_launchable_shortcut(Path::new("Signal.exe")));
        assert!(!is_launchable_shortcut(Path::new("notes.txt")));
        assert!(!is_launchable_shortcut(Path::new("Signal")));
    }

    #[test]
    fn app_collection_recurses_and_skips_non_launchable_files() {
        let root = test_dir("shortcuts");
        let nested = root.join("Utilities");
        fs::create_dir_all(&nested).expect("nested test directory should be created");
        fs::write(root.join("Signal.lnk"), b"").expect("shortcut should be written");
        fs::write(nested.join("Nested App.url"), b"").expect("url shortcut should be written");
        fs::write(root.join("notes.txt"), b"").expect("ignored file should be written");

        let mut apps = Vec::new();
        collect_apps_from_dir(&root, AppSource::UserStartMenu, &mut apps);
        let mut names = apps.iter().map(|app| app.name.clone()).collect::<Vec<_>>();
        names.sort();

        assert_eq!(names, vec!["Nested App".to_string(), "Signal".to_string()]);
        assert!(apps
            .iter()
            .all(|app| app.source == AppSource::UserStartMenu));

        let _ = fs::remove_dir_all(root);
    }
}
