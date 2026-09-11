use mega_win_alt_tab::core::{AppEntry, AppSource};
use serde::Deserialize;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use windows::core::{w, Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HWND};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_READ, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
use windows::Win32::UI::Shell::{
    ApplicationActivationManager, IApplicationActivationManager, IShellLinkW, ShellExecuteW,
    ShellLink, AO_NONE,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const APP_PATHS_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\App Paths");
const APP_USER_MODEL_ID_PREFIX: &str = "appusermodelid:";

pub(super) fn enumerate_apps() -> Vec<AppEntry> {
    let mut apps = Vec::new();
    for (root, source) in app_search_roots() {
        collect_apps_from_dir(&root, source, &mut apps);
    }
    unsafe {
        collect_apps_from_app_paths(HKEY_CURRENT_USER, AppSource::UserAppPath, &mut apps);
        collect_apps_from_app_paths(HKEY_LOCAL_MACHINE, AppSource::MachineAppPath, &mut apps);
    }
    collect_start_apps(&mut apps);
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

        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Some(launch_identity) = shortcut_app_launch_identity(&path) else {
            continue;
        };

        apps.push(AppEntry {
            name: name.to_string(),
            launch_path: path.to_string_lossy().to_string(),
            launch_identity,
            source,
        });
    }
}

fn shortcut_app_launch_identity(path: &Path) -> Option<Option<String>> {
    if !is_start_menu_app_shortcut_type(path) {
        return None;
    }

    let extension = path.extension().and_then(|extension| extension.to_str())?;
    if extension.eq_ignore_ascii_case("appref-ms") {
        return Some(None);
    }

    unsafe { shell_link_target(path) }
        .filter(|target| is_executable_launch_target(target))
        .map(Some)
}

fn is_start_menu_app_shortcut_type(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("lnk") || extension.eq_ignore_ascii_case("appref-ms")
        })
}

fn is_executable_launch_target(target: &str) -> bool {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with("shell:") {
        return true;
    }

    Path::new(trimmed)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "exe" | "com" | "bat" | "cmd" | "ps1"
            )
        })
}

unsafe fn shell_link_target(path: &Path) -> Option<String> {
    let _apartment = ComApartment::initialize()?;
    let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
    let persist_file: IPersistFile = shell_link.cast().ok()?;
    let path_wide = to_wide_z(path.to_string_lossy().as_ref());
    persist_file
        .Load(PCWSTR(path_wide.as_ptr()), STGM_READ)
        .ok()?;

    let mut target = vec![0u16; 32768];
    shell_link
        .GetPath(&mut target, std::ptr::null_mut(), 0)
        .ok()?;
    let target = utf16z_to_string(&target);
    (!target.trim().is_empty()).then_some(target)
}

struct ComApartment;

impl ComApartment {
    unsafe fn initialize() -> Option<Self> {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .is_ok()
            .then_some(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

unsafe fn collect_apps_from_app_paths(root: HKEY, source: AppSource, apps: &mut Vec<AppEntry>) {
    let mut app_paths = HKEY::default();
    if RegOpenKeyExW(root, APP_PATHS_KEY, 0, KEY_READ, &mut app_paths) != ERROR_SUCCESS {
        return;
    }

    let mut index = 0;
    while let Some(key_name) = enum_registry_subkey(app_paths, index) {
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

        if let Some(launch_path) = read_registry_string(app_key, None) {
            if !launch_path.trim().is_empty() {
                apps.push(AppEntry {
                    name: app_name_from_app_path_key(&key_name),
                    launch_identity: Some(launch_path.clone()),
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

unsafe fn read_registry_string(key: HKEY, value_name: Option<&str>) -> Option<String> {
    let value_name_wide = value_name.map(to_wide_z);
    let value_name = value_name_wide
        .as_ref()
        .map_or(PCWSTR::null(), |name| PCWSTR(name.as_ptr()));
    let mut value_type = REG_VALUE_TYPE::default();
    let mut byte_len = 0u32;
    let status = RegQueryValueExW(
        key,
        value_name,
        None,
        Some(&mut value_type),
        None,
        Some(&mut byte_len),
    );
    if status != ERROR_SUCCESS || !matches!(value_type, REG_SZ | REG_EXPAND_SZ) {
        return None;
    }

    let mut bytes = vec![0u8; byte_len as usize];
    let status = RegQueryValueExW(
        key,
        value_name,
        None,
        Some(&mut value_type),
        Some(bytes.as_mut_ptr()),
        Some(&mut byte_len),
    );
    if status != ERROR_SUCCESS || !matches!(value_type, REG_SZ | REG_EXPAND_SZ) {
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
    if bytes.len() & 1 == 1 {
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

#[derive(Deserialize)]
struct StartAppEntry {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "AppID")]
    app_id: String,
}

fn collect_start_apps(apps: &mut Vec<AppEntry>) {
    for entry in start_app_entries() {
        let name = entry.name.trim();
        let app_id = entry.app_id.trim();
        if name.is_empty() || !is_start_app_launch_id(app_id) {
            continue;
        }

        let launch_path = start_app_launch_path(name, app_id);
        apps.push(AppEntry {
            name: name.to_string(),
            launch_identity: Some(launch_path.clone()),
            launch_path,
            source: AppSource::PackagedApp,
        });
    }
}

fn start_app_entries() -> Vec<StartAppEntry> {
    let script = r#"
$apps = @(Get-StartApps | Select-Object Name,AppID)
ConvertTo-Json -InputObject $apps -Compress
"#;
    let Ok(output) = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .creation_flags(CREATE_NO_WINDOW.0)
        .output()
    else {
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    let json = String::from_utf8_lossy(&output.stdout);
    parse_start_app_entries(json.trim())
}

fn parse_start_app_entries(json: &str) -> Vec<StartAppEntry> {
    if json.is_empty() || json == "null" {
        return Vec::new();
    }

    serde_json::from_str::<Vec<StartAppEntry>>(json).unwrap_or_default()
}

fn is_start_app_launch_id(app_id: &str) -> bool {
    let app_id = app_id.trim();
    if app_id.is_empty() {
        return false;
    }

    if is_executable_launch_target(app_id) {
        return true;
    }

    if app_id.contains("://") {
        return true;
    }

    if app_id.starts_with('{')
        || app_id.contains('\\')
        || app_id.contains('/')
        || app_id.starts_with("Microsoft.AutoGenerated.")
    {
        return false;
    }

    true
}

fn start_app_launch_path(_name: &str, app_id: &str) -> String {
    if is_absolute_executable_launch_target(app_id) || app_id.contains("://") {
        return app_id.to_string();
    }

    app_user_model_launch_path(app_id)
}

fn app_user_model_launch_path(app_id: &str) -> String {
    format!("{APP_USER_MODEL_ID_PREFIX}{app_id}")
}

fn app_user_model_id_from_launch_path(launch_path: &str) -> Option<&str> {
    launch_path
        .trim()
        .strip_prefix(APP_USER_MODEL_ID_PREFIX)
        .filter(|app_id| !app_id.trim().is_empty())
}

fn is_absolute_executable_launch_target(target: &str) -> bool {
    let path = Path::new(target.trim());
    path.is_absolute() && is_executable_launch_target(target)
}

pub(super) unsafe fn launch_app(hwnd: HWND, launch_path: &str) -> bool {
    if let Some(app_id) = app_user_model_id_from_launch_path(launch_path) {
        return activate_app_user_model_id(app_id) || launch_shell_apps_folder_app_id(hwnd, app_id);
    }

    let file = to_wide_z(launch_path);
    let directory = launch_working_directory(launch_path);
    let directory_wide = directory.as_deref().map(to_wide_z);
    let directory = directory_wide
        .as_ref()
        .map_or(PCWSTR::null(), |path| PCWSTR(path.as_ptr()));
    let result = ShellExecuteW(
        hwnd,
        w!("open"),
        PCWSTR(file.as_ptr()),
        PCWSTR::null(),
        directory,
        SW_SHOWNORMAL,
    );
    result.0 as isize > 32
}

unsafe fn launch_shell_apps_folder_app_id(hwnd: HWND, app_id: &str) -> bool {
    let file = to_wide_z("explorer.exe");
    let parameters = to_wide_z(&format!("shell:AppsFolder\\{}", app_id.trim()));
    let result = ShellExecuteW(
        hwnd,
        w!("open"),
        PCWSTR(file.as_ptr()),
        PCWSTR(parameters.as_ptr()),
        PCWSTR::null(),
        SW_SHOWNORMAL,
    );
    result.0 as isize > 32
}

unsafe fn activate_app_user_model_id(app_id: &str) -> bool {
    let _apartment = ComApartment::initialize();
    let activation_manager: IApplicationActivationManager =
        match CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_INPROC_SERVER) {
            Ok(manager) => manager,
            Err(_) => return false,
        };

    let app_id = to_wide_z(app_id.trim());
    activation_manager
        .ActivateApplication(PCWSTR(app_id.as_ptr()), PCWSTR::null(), AO_NONE)
        .is_ok()
}

fn launch_working_directory(launch_path: &str) -> Option<String> {
    let path = Path::new(launch_path.trim());
    if !path.is_absolute() || !is_executable_launch_target(launch_path) {
        return None;
    }

    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.to_string_lossy().to_string())
}

fn to_wide_z(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn utf16z_to_string(value: &[u16]) -> String {
    let len = value.iter().position(|ch| *ch == 0).unwrap_or(value.len());
    String::from_utf16_lossy(&value[..len])
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
    fn shortcut_type_filter_accepts_app_shortcut_file_types() {
        assert!(is_start_menu_app_shortcut_type(Path::new("Signal.lnk")));
        assert!(is_start_menu_app_shortcut_type(Path::new(
            "ClickOnce.APPREF-MS"
        )));
        assert!(!is_start_menu_app_shortcut_type(Path::new("Website.URL")));
        assert!(!is_start_menu_app_shortcut_type(Path::new("Signal.exe")));
        assert!(!is_start_menu_app_shortcut_type(Path::new("notes.txt")));
        assert!(!is_start_menu_app_shortcut_type(Path::new("Signal")));
    }

    #[test]
    fn executable_target_filter_rejects_help_and_document_targets() {
        assert!(is_executable_launch_target(
            r"C:\Program Files\Signal\Signal.exe"
        ));
        assert!(is_executable_launch_target(r"C:\Tools\launcher.cmd"));
        assert!(is_executable_launch_target(
            r"shell:AppsFolder\Example.App!App"
        ));
        assert!(!is_executable_launch_target(
            r"C:\Program Files\7-Zip\7-zip.chm"
        ));
        assert!(!is_executable_launch_target(
            r"C:\Program Files\App\readme.txt"
        ));
        assert!(!is_executable_launch_target(
            r"C:\Program Files\App\manual.pdf"
        ));
        assert!(!is_executable_launch_target(" "));
    }

    #[test]
    fn app_collection_recurses_and_skips_non_launchable_files() {
        let root = test_dir("shortcuts");
        let nested = root.join("Utilities");
        fs::create_dir_all(&nested).expect("nested test directory should be created");
        fs::write(root.join("Signal.appref-ms"), b"").expect("shortcut should be written");
        fs::write(nested.join("Nested App.appref-ms"), b"").expect("shortcut should be written");
        fs::write(nested.join("Website.url"), b"").expect("url shortcut should be written");
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

    #[test]
    fn start_app_ids_launch_through_app_user_model_activation() {
        let packaged_app_id = "5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App";
        let docker_app_id = "Docker.DockerForWindows.Settings";

        assert!(is_start_app_launch_id(packaged_app_id));
        assert!(is_start_app_launch_id(docker_app_id));
        assert!(is_start_app_launch_id(
            r"C:\Program Files\Docker\Docker\Docker Desktop.exe"
        ));
        assert!(is_start_app_launch_id("steam://rungameid/400020"));
        assert_eq!(
            start_app_launch_path("WhatsApp", packaged_app_id),
            "appusermodelid:5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App"
        );
        assert_eq!(
            start_app_launch_path("Docker Desktop", docker_app_id),
            "appusermodelid:Docker.DockerForWindows.Settings"
        );
        assert_eq!(
            start_app_launch_path("Steam Game", "steam://rungameid/400020"),
            "steam://rungameid/400020"
        );
        assert_eq!(
            start_app_launch_path(
                "Docker Desktop",
                r"C:\Program Files\Docker\Docker\Docker Desktop.exe"
            ),
            r"C:\Program Files\Docker\Docker\Docker Desktop.exe"
        );
        assert!(!is_start_app_launch_id(
            r"{6D809377-6AF0-444B-8957-A3773F02200E}\IrfanView\i_changes.txt"
        ));
        assert!(!is_start_app_launch_id(
            r"{6D809377-6AF0-444B-8957-A3773F02200E}\7-Zip\7-zip.chm"
        ));
        assert!(!is_start_app_launch_id(
            "Microsoft.AutoGenerated.{8ABD94FB-E7D6-84A6-A997-C918EDDE0AE5}"
        ));
    }

    #[test]
    fn app_user_model_launch_path_round_trips_internal_prefix() {
        let launch_path = app_user_model_launch_path("Docker.DockerForWindows.Settings");

        assert_eq!(
            app_user_model_id_from_launch_path(&launch_path),
            Some("Docker.DockerForWindows.Settings")
        );
        assert_eq!(app_user_model_id_from_launch_path(""), None);
        assert_eq!(
            app_user_model_id_from_launch_path("C:\\Tools\\App.exe"),
            None
        );
    }

    #[test]
    fn executable_launches_use_parent_working_directory() {
        assert_eq!(
            launch_working_directory(r"C:\Program Files\Docker\Docker\Docker Desktop.exe"),
            Some(r"C:\Program Files\Docker\Docker".to_string())
        );
        assert_eq!(
            launch_working_directory("shell:AppsFolder\\Example.App!App"),
            None
        );
        assert_eq!(launch_working_directory("steam://rungameid/400020"), None);
    }

    #[test]
    fn start_app_entries_parse_json_array() {
        let apps = parse_start_app_entries(
            r#"[{"Name":"WhatsApp","AppID":"5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App"}]"#,
        );

        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "WhatsApp");
        assert_eq!(apps[0].app_id, "5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App");
    }
}
