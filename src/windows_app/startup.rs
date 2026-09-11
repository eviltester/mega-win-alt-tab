use std::env;
use std::path::Path;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegEnumValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SAM_FLAGS, REG_SZ,
    REG_VALUE_TYPE,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Mega Win Alt Tab");
const RUN_VALUE_NAME: &str = "Mega Win Alt Tab";
const STARTUP_ARG: &str = "--startup";
const MAX_RUN_VALUE_NAME_CHARS: usize = 16_384;
const MAX_RUN_VALUE_BYTES: usize = 65_536;

const GENERATED_STARTUP_PATTERNS: &[GeneratedStartupPattern] = &[
    GeneratedStartupPattern::new("mega-win-alt-tab-v", "-windows-x64.exe"),
    GeneratedStartupPattern::new("mega-win-alt-tab-v", ".exe"),
    GeneratedStartupPattern::new("mega-win-alt-tab-", "-windows-x64.exe"),
    GeneratedStartupPattern::new("Mega Win Alt Tab v", ""),
    GeneratedStartupPattern::new("MegaWinAltTab v", ""),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LegacyStartupEntry {
    pub(super) name: String,
    pub(super) command: String,
}

struct GeneratedStartupPattern {
    prefix: &'static str,
    suffix: &'static str,
}

impl GeneratedStartupPattern {
    const fn new(prefix: &'static str, suffix: &'static str) -> Self {
        Self { prefix, suffix }
    }
}

pub(super) fn is_run_at_startup_enabled() -> bool {
    let Some(expected) = current_startup_command() else {
        return false;
    };

    unsafe { read_run_value().is_some_and(|actual| actual == expected) }
}

pub(super) fn set_run_at_startup(enabled: bool) -> bool {
    let Some(command) = current_startup_command() else {
        return false;
    };

    unsafe {
        if enabled {
            write_run_value(&command)
        } else {
            delete_run_value()
        }
    }
}

pub(super) fn legacy_startup_entries() -> Vec<LegacyStartupEntry> {
    let current_command = current_startup_command();
    unsafe {
        enumerate_run_values()
            .into_iter()
            .filter(|entry| is_legacy_startup_entry(entry, current_command.as_deref()))
            .collect()
    }
}

pub(super) fn remove_startup_entries(entries: &[LegacyStartupEntry]) -> bool {
    if entries.is_empty() {
        return true;
    }

    unsafe {
        let Some(key) = open_run_key(REG_SAM_FLAGS(KEY_READ.0 | KEY_SET_VALUE.0)) else {
            return false;
        };

        let mut removed_all = true;
        for entry in entries {
            let name = wide_null(&entry.name);
            let status = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
            if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
                removed_all = false;
            }
        }

        let _ = RegCloseKey(key);
        removed_all
    }
}

pub(super) fn current_startup_command() -> Option<String> {
    env::current_exe()
        .ok()
        .map(|path| startup_command_for_exe(path.as_path()))
}

fn startup_command_for_exe(exe_path: &Path) -> String {
    format!("\"{}\" {STARTUP_ARG}", exe_path.display())
}

unsafe fn open_run_key(access: REG_SAM_FLAGS) -> Option<HKEY> {
    let mut key = HKEY::default();
    let status = RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, access, &mut key);
    if status == ERROR_SUCCESS {
        Some(key)
    } else {
        None
    }
}

unsafe fn read_run_value() -> Option<String> {
    let key = open_run_key(KEY_READ)?;
    let result = read_registry_string_value(key);
    let _ = RegCloseKey(key);
    result
}

unsafe fn enumerate_run_values() -> Vec<LegacyStartupEntry> {
    let Some(key) = open_run_key(KEY_READ) else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    let mut index = 0;
    loop {
        let mut name_buffer = vec![0u16; MAX_RUN_VALUE_NAME_CHARS];
        let mut value_name_len = name_buffer.len() as u32;
        let mut value_type = 0u32;
        let mut data = vec![0u8; MAX_RUN_VALUE_BYTES];
        let mut data_len = data.len() as u32;

        let status = RegEnumValueW(
            key,
            index,
            PWSTR(name_buffer.as_mut_ptr()),
            &mut value_name_len,
            None,
            Some(&mut value_type),
            Some(data.as_mut_ptr()),
            Some(&mut data_len),
        );
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        index += 1;

        if status != ERROR_SUCCESS || !is_string_registry_type(REG_VALUE_TYPE(value_type)) {
            continue;
        }

        let name = String::from_utf16(&name_buffer[..value_name_len as usize]);
        let command = registry_bytes_to_string(&data[..data_len as usize]);
        if let (Ok(name), Some(command)) = (name, command) {
            entries.push(LegacyStartupEntry { name, command });
        }
    }

    let _ = RegCloseKey(key);
    entries
}

unsafe fn read_registry_string_value(key: HKEY) -> Option<String> {
    let mut value_type = REG_VALUE_TYPE::default();
    let mut byte_len = 0u32;
    let status = RegQueryValueExW(
        key,
        RUN_VALUE,
        None,
        Some(&mut value_type),
        None,
        Some(&mut byte_len),
    );
    if status == ERROR_FILE_NOT_FOUND || status != ERROR_SUCCESS || value_type != REG_SZ {
        return None;
    }

    let mut bytes = vec![0u8; byte_len as usize];
    let status = RegQueryValueExW(
        key,
        RUN_VALUE,
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

fn is_string_registry_type(value_type: REG_VALUE_TYPE) -> bool {
    value_type == REG_SZ || value_type == REG_EXPAND_SZ
}

unsafe fn write_run_value(command: &str) -> bool {
    let Some(key) = open_run_key(REG_SAM_FLAGS(KEY_READ.0 | KEY_SET_VALUE.0)) else {
        return false;
    };

    let wide = command
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
    let status = RegSetValueExW(key, RUN_VALUE, 0, REG_SZ, Some(bytes));
    let _ = RegCloseKey(key);
    status == ERROR_SUCCESS
}

unsafe fn delete_run_value() -> bool {
    let Some(key) = open_run_key(REG_SAM_FLAGS(KEY_READ.0 | KEY_SET_VALUE.0)) else {
        return false;
    };

    let status = RegDeleteValueW(key, RUN_VALUE);
    let _ = RegCloseKey(key);
    status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND
}

fn matches_generated_startup_convention(value_name: &str, command: &str) -> bool {
    matches_generated_name(value_name)
        || command_exe_name(command).is_some_and(matches_generated_name)
}

fn is_legacy_startup_entry(
    entry: &LegacyStartupEntry,
    current_startup_command: Option<&str>,
) -> bool {
    entry.name != RUN_VALUE_NAME
        && current_startup_command != Some(entry.command.as_str())
        && matches_generated_startup_convention(&entry.name, &entry.command)
}

fn matches_generated_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    GENERATED_STARTUP_PATTERNS
        .iter()
        .any(|pattern| matches_pattern(&lower, pattern))
}

fn matches_pattern(name: &str, pattern: &GeneratedStartupPattern) -> bool {
    let prefix = pattern.prefix.to_ascii_lowercase();
    let suffix = pattern.suffix.to_ascii_lowercase();
    let Some(remainder) = name.strip_prefix(&prefix) else {
        return false;
    };
    let version = if suffix.is_empty() {
        remainder
    } else {
        let Some(version) = remainder.strip_suffix(&suffix) else {
            return false;
        };
        version
    };

    looks_like_version(version)
}

fn looks_like_version(value: &str) -> bool {
    let mut saw_digit = false;
    let mut saw_dot = false;
    for ch in value.chars() {
        match ch {
            '0'..='9' => saw_digit = true,
            '.' => saw_dot = true,
            '-' | '+' => {}
            'a'..='z' | 'A'..='Z' => {}
            _ => return false,
        }
    }

    saw_digit && saw_dot
}

fn command_exe_name(command: &str) -> Option<&str> {
    let command = command.trim();
    let path = if let Some(rest) = command.strip_prefix('"') {
        rest.split_once('"')?.0
    } else {
        command.split_whitespace().next()?
    };

    path.rsplit(['\\', '/']).next()
}

fn wide_null(value: &str) -> Vec<u16> {
    value
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn startup_command_quotes_executable_and_adds_startup_flag() {
        let path = PathBuf::from(r"C:\Tools\Mega Win Alt Tab\mega-win-alt-tab.exe");

        assert_eq!(
            startup_command_for_exe(&path),
            r#""C:\Tools\Mega Win Alt Tab\mega-win-alt-tab.exe" --startup"#
        );
    }

    #[test]
    fn registry_string_decoder_trims_null_terminator() {
        let bytes = "abc\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();

        assert_eq!(registry_bytes_to_string(&bytes), Some("abc".to_string()));
    }

    #[test]
    fn registry_string_decoder_rejects_odd_byte_count() {
        assert_eq!(registry_bytes_to_string(b"a"), None);
    }

    #[test]
    fn generated_startup_detection_matches_release_asset_conventions() {
        assert!(matches_generated_startup_convention(
            "Some old release",
            r#""C:\Downloads\mega-win-alt-tab-v1.2.3-windows-x64.exe" --startup"#
        ));
        assert!(matches_generated_startup_convention(
            "mega-win-alt-tab-v1.2.3-windows-x64.exe",
            r#""C:\Other\anything.exe" --startup"#
        ));
        assert!(matches_generated_startup_convention(
            "Mega Win Alt Tab v1.2.3",
            r#""C:\Other\anything.exe" --startup"#
        ));
    }

    #[test]
    fn generated_startup_detection_ignores_unversioned_current_name() {
        assert!(!matches_generated_startup_convention(
            "Mega Win Alt Tab",
            r#""C:\Tools\mega-win-alt-tab.exe" --startup"#
        ));
        assert!(!matches_generated_startup_convention(
            "Other App",
            r#""C:\Tools\other.exe" --startup"#
        ));
    }

    #[test]
    fn legacy_startup_entry_ignores_current_registry_value_name() {
        let entry = LegacyStartupEntry {
            name: RUN_VALUE_NAME.to_string(),
            command: r#""C:\Downloads\mega-win-alt-tab-v1.0.0-windows-x64.exe" --startup"#
                .to_string(),
        };

        assert!(!is_legacy_startup_entry(&entry, None));
    }

    #[test]
    fn legacy_startup_entry_ignores_current_startup_command() {
        let command = r#""C:\Downloads\mega-win-alt-tab-v1.0.0-windows-x64.exe" --startup"#;
        let entry = LegacyStartupEntry {
            name: "mega-win-alt-tab-v1.0.0-windows-x64.exe".to_string(),
            command: command.to_string(),
        };

        assert!(!is_legacy_startup_entry(&entry, Some(command)));
    }

    #[test]
    fn command_exe_name_handles_quoted_and_unquoted_commands() {
        assert_eq!(
            command_exe_name(r#""C:\Downloads\mega-win-alt-tab-v1.0.0-windows-x64.exe" --startup"#),
            Some("mega-win-alt-tab-v1.0.0-windows-x64.exe")
        );
        assert_eq!(
            command_exe_name(r"C:\Tools\mega-win-alt-tab-v1.0.0.exe --startup"),
            Some("mega-win-alt-tab-v1.0.0.exe")
        );
    }
}
