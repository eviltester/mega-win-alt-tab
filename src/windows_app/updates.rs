use serde::Deserialize;
use std::cmp::Ordering;
use std::ffi::c_void;
use std::ptr::null_mut;
use windows::core::{w, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_FLAG_SECURE,
};

const GITHUB_HOST: PCWSTR = w!("api.github.com");
const GITHUB_LATEST_RELEASE_PATH: PCWSTR = w!("/repos/eviltester/mega-win-alt-tab/releases/latest");
const HTTPS_PORT: u16 = 443;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct UpdateInfo {
    pub(super) latest_version: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
}

struct WinHttpHandle(*mut c_void);

impl WinHttpHandle {
    fn new(handle: *mut c_void) -> Option<Self> {
        (!handle.is_null()).then_some(Self(handle))
    }
}

impl Drop for WinHttpHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

pub(super) fn check_for_update(current_version: &str) -> Option<UpdateInfo> {
    let json = unsafe { fetch_latest_release_json()? };
    update_info_from_latest_release_json(&json, current_version)
}

fn update_info_from_latest_release_json(json: &str, current_version: &str) -> Option<UpdateInfo> {
    let release = serde_json::from_str::<GithubRelease>(json).ok()?;
    let latest_version = release.tag_name.trim();
    if !is_release_newer(latest_version, current_version) {
        return None;
    }

    Some(UpdateInfo {
        latest_version: latest_version.to_string(),
    })
}

unsafe fn fetch_latest_release_json() -> Option<String> {
    let session = WinHttpHandle::new(WinHttpOpen(
        w!("Mega Win Alt Tab Update Check"),
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
        PCWSTR::null(),
        PCWSTR::null(),
        0,
    ))?;
    let connection = WinHttpHandle::new(WinHttpConnect(session.0, GITHUB_HOST, HTTPS_PORT, 0))?;
    let request = WinHttpHandle::new(WinHttpOpenRequest(
        connection.0,
        w!("GET"),
        GITHUB_LATEST_RELEASE_PATH,
        PCWSTR::null(),
        PCWSTR::null(),
        std::ptr::null(),
        WINHTTP_FLAG_SECURE,
    ))?;

    let headers = "Accept: application/vnd.github+json\r\nUser-Agent: mega-win-alt-tab\r\nX-GitHub-Api-Version: 2022-11-28\r\n"
        .encode_utf16()
        .collect::<Vec<_>>();
    WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0).ok()?;
    WinHttpReceiveResponse(request.0, null_mut()).ok()?;

    let mut response = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let mut bytes_read = 0u32;
        WinHttpReadData(
            request.0,
            buffer.as_mut_ptr().cast::<c_void>(),
            buffer.len() as u32,
            &mut bytes_read,
        )
        .ok()?;

        if bytes_read == 0 {
            break;
        }
        response.extend_from_slice(&buffer[..bytes_read as usize]);
    }

    String::from_utf8(response).ok()
}

fn is_release_newer(latest_version: &str, current_version: &str) -> bool {
    compare_versions(latest_version, current_version) == Ordering::Greater
}

fn compare_versions(left: &str, right: &str) -> Ordering {
    let left_parts = version_parts(left);
    let right_parts = version_parts(right);
    let part_count = left_parts.len().max(right_parts.len());

    for index in 0..part_count {
        let left = *left_parts.get(index).unwrap_or(&0);
        let right = *right_parts.get(index).unwrap_or(&0);
        match left.cmp(&right) {
            Ordering::Equal => {}
            order => return order,
        }
    }

    Ordering::Equal
}

fn version_parts(value: &str) -> Vec<u64> {
    let value = value.trim().trim_start_matches('v').trim_start_matches('V');
    value
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u64>().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare_handles_v_prefix_and_numeric_ordering() {
        assert!(is_release_newer("v1.10.0", "1.2.0"));
        assert!(is_release_newer("1.0.1", "1.0.0"));
        assert!(!is_release_newer("v1.0.0", "1.0.0"));
        assert!(!is_release_newer("0.9.9", "1.0.0"));
    }

    #[test]
    fn latest_release_json_reports_update_only_for_newer_versions() {
        let json = r#"{"tag_name":"v1.2.3"}"#;

        assert_eq!(
            update_info_from_latest_release_json(json, "1.2.2"),
            Some(UpdateInfo {
                latest_version: "v1.2.3".to_string(),
            })
        );
        assert_eq!(update_info_from_latest_release_json(json, "1.2.3"), None);
    }
}
