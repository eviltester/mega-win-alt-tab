use mega_win_alt_tab::core::{AppEntry, AppSource};
use std::fs;
use std::path::{Path, PathBuf};
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HWND};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const APP_PATHS_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\App Paths");

pub(super) fn enumerate_apps() -> Vec<AppEntry> {
    let mut apps = Vec::new();
    for (root, source) in app_search_roots() {
        collect_apps_from_dir(&root, source, &mut apps);
    }
    unsafe {
        collect_apps_from_app_paths(HKEY_CURRENT_USER, AppSource::UserAppPath, &mut apps);
        collect_apps_from_app_paths(HKEY_LOCAL_MACHINE, AppSource::MachineAppPath, &mut apps);
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

unsafe fn collect_apps_from_app_paths(root: HKEY, source: AppSource, apps: &mut Vec<AppEntry>) {
    let mut app_paths = HKEY::default();
    if RegOpenKeyExW(root, APP_PATHS_KEY, 0, KEY_READ, &mut app_paths) != ERROR_SUCCESS {
        return;
    }

    let mut index = 0;
    loop {
        let Some(key_name) = enum_registry_subkey(app_paths, index) else {
            break;
        };
        index += 1;

        let key_name_wide = to_wide_z(&key_name);
        let mut app_key = HKEY::default();
        if RegOpenKeyExW(
            app_paths,
            PCWSTR(key_name_wide.as_ptr()),
            0,
            KEY_READ,
            &mut app_key,
        ) != ERROR_SUCCESS
        {
            continue;
        }

        if let Some(launch_path) = read_default_registry_string(app_key) {
            if !launch_path.trim().is_empty() {
                apps.push(AppEntry {
                    name: app_name_from_app_path_key(&key_name),
                    launch_path,
                    source,
                });
            }
        }

        let _ = RegCloseKey(app_key);
    }

    let _ = RegCloseKey(app_paths);
}

unsafe fn enum_registry_subkey(key: HKEY, index: u32) -> Option<String> {
    let mut capacity = 260usize;
    loop {
        let mut name = vec![0u16; capacity];
        let mut name_len = name.len() as u32;
        let status = RegEnumKeyExW(
            key,
            index,
            PWSTR(name.as_mut_ptr()),
            &mut name_len,
            None,
            PWSTR::null(),
            None,
            None,
        );

        if status == ERROR_SUCCESS {
            return Some(String::from_utf16_lossy(&name[..name_len as usize]));
        }
        if status == ERROR_MORE_DATA {
            capacity *= 2;
            continue;
        }
        if status == ERROR_NO_MORE_ITEMS {
            return None;
        }
        return None;
    }
}

unsafe fn read_default_registry_string(key: HKEY) -> Option<String> {
    let mut value_type = REG_VALUE_TYPE::default();
    let mut byte_len = 0u32;
    let status = RegQueryValueExW(
        key,
        PCWSTR::null(),
        None,
        Some(&mut value_type),
        None,
        Some(&mut byte_len),
    );
    if status != ERROR_SUCCESS || value_type != REG_SZ {
        return None;
    }

    let mut bytes = vec![0u8; byte_len as usize];
    let status = RegQueryValueExW(
        key,
        PCWSTR::null(),
        None,
        Some(&mut value_type),
        Some(bytes.as_mut_ptr()),
        Some(&mut byte_len),
    );
    if status != ERROR_SUCCESS || value_type != REG_SZ {
        return None;
    }

    registry_bytes_to_string(&bytes[..byte_len as usize])
}

fn app_name_from_app_path_key(key_name: &str) -> String {
    Path::new(key_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(pretty_app_name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| key_name.to_string())
}

fn pretty_app_name(value: &str) -> String {
    value
        .replace(['_', '-'], " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first
                    .to_uppercase()
                    .chain(chars.flat_map(char::to_lowercase))
                    .collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn registry_bytes_to_string(bytes: &[u8]) -> Option<String> {
    if bytes.len() % 2 != 0 {
        return None;
    }

    let mut units = Vec::with_capacity(bytes.len() / 2);
    let mut index = 0;
    while index < bytes.len() {
        units.push(u16::from_le_bytes([bytes[index], bytes[index + 1]]));
        index += 2;
    }

    while units.last() == Some(&0) {
        units.pop();
    }

    String::from_utf16(&units).ok()
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

    #[test]
    fn app_path_key_names_become_searchable_app_names() {
        assert_eq!(app_name_from_app_path_key("thunderbird.exe"), "Thunderbird");
        assert_eq!(app_name_from_app_path_key("some-tool.exe"), "Some Tool");
    }

    #[test]
    fn registry_string_decoder_trims_null_terminator() {
        let bytes = "C:\\Tools\\thunderbird.exe\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();

        assert_eq!(
            registry_bytes_to_string(&bytes),
            Some(r"C:\Tools\thunderbird.exe".to_string())
        );
    }
}
