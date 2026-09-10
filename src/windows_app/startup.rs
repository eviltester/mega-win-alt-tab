use std::env;
use std::path::Path;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Mega Win Alt Tab");
const STARTUP_ARG: &str = "--startup";

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
}
