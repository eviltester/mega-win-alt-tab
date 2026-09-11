use mega_win_alt_tab::core::{dedupe_favorite_folders, display_folder_path, FavoriteFolderEntry};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{DragFinish, DragQueryFileW, ShellExecuteW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const FAVORITE_FOLDERS_FILE: &str = "favorite-folders.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FavoriteFolderAddResult {
    pub(super) entry: FavoriteFolderEntry,
    pub(super) added: bool,
}

#[derive(Serialize, Deserialize)]
struct StoredFavoriteFolder {
    path: String,
}

pub(super) fn load_favorite_folders() -> Vec<FavoriteFolderEntry> {
    load_favorite_folders_from_file(&favorite_folders_path())
}

pub(super) fn save_favorite_folders(folders: &[FavoriteFolderEntry]) -> bool {
    save_favorite_folders_to_file(&favorite_folders_path(), folders).is_ok()
}

pub(super) fn add_favorite_folder_path(
    folders: &mut Vec<FavoriteFolderEntry>,
    path: &Path,
) -> Option<FavoriteFolderAddResult> {
    let entry = favorite_folder_from_path(path)?;
    if let Some(existing) = folders
        .iter()
        .find(|folder| folder.normalized_path == entry.normalized_path)
        .cloned()
    {
        return Some(FavoriteFolderAddResult {
            entry: existing,
            added: false,
        });
    }

    folders.push(entry.clone());
    *folders = dedupe_favorite_folders(folders);
    Some(FavoriteFolderAddResult { entry, added: true })
}

pub(super) fn remove_favorite_folder_path(
    folders: &mut Vec<FavoriteFolderEntry>,
    path: &str,
) -> bool {
    let normalized = normalize_folder_path(path);
    let original_len = folders.len();
    folders.retain(|folder| folder.normalized_path != normalized);
    folders.len() != original_len
}

pub(super) unsafe fn dropped_file_paths(hdrop: HDROP) -> Vec<PathBuf> {
    let count = DragQueryFileW(hdrop, u32::MAX, None);
    let mut paths = Vec::new();
    for index in 0..count {
        let len = DragQueryFileW(hdrop, index, None);
        if len == 0 {
            continue;
        }
        let mut buffer = vec![0u16; len as usize + 1];
        let read = DragQueryFileW(hdrop, index, Some(&mut buffer));
        if read == 0 {
            continue;
        }
        paths.push(PathBuf::from(String::from_utf16_lossy(
            &buffer[..read as usize],
        )));
    }
    DragFinish(hdrop);
    paths
}

pub(super) unsafe fn open_folder(hwnd: HWND, path: &str) -> bool {
    let file = to_wide_z(path);
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

pub(super) fn normalize_folder_path(path: &str) -> String {
    let mut normalized = display_folder_path(path).replace('/', "\\").to_lowercase();
    while normalized.len() > 3 && normalized.ends_with('\\') {
        normalized.pop();
    }
    normalized
}

fn load_favorite_folders_from_file(path: &Path) -> Vec<FavoriteFolderEntry> {
    let Ok(json) = fs::read_to_string(path) else {
        return Vec::new();
    };
    if json.trim().is_empty() {
        return Vec::new();
    }

    let Ok(stored) = serde_json::from_str::<Vec<StoredFavoriteFolder>>(&json) else {
        return Vec::new();
    };

    dedupe_favorite_folders(
        &stored
            .iter()
            .filter_map(|folder| favorite_folder_from_path(Path::new(&folder.path)))
            .collect::<Vec<_>>(),
    )
}

fn save_favorite_folders_to_file(path: &Path, folders: &[FavoriteFolderEntry]) -> io::Result<()> {
    let folders = dedupe_favorite_folders(folders);
    let stored = folders
        .iter()
        .map(|folder| StoredFavoriteFolder {
            path: folder.path.clone(),
        })
        .collect::<Vec<_>>();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&stored)?;
    fs::write(path, json)
}

fn favorite_folder_from_path(path: &Path) -> Option<FavoriteFolderEntry> {
    if !path.is_dir() {
        return None;
    }
    let absolute = path.canonicalize().ok()?;
    let path = absolute.to_string_lossy().to_string();
    let name = absolute
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| path.clone());

    Some(FavoriteFolderEntry {
        name,
        normalized_path: normalize_folder_path(&path),
        path,
    })
}

fn favorite_folders_path() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("MegaWinAltTab")
        .join(FAVORITE_FOLDERS_FILE)
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
            "mega-win-alt-tab-folders-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("test directory should be created");
        dir
    }

    #[test]
    fn favorite_folder_normalizes_paths_case_and_separators() {
        assert_eq!(
            normalize_folder_path(r"C:/Users/Example/Downloads/"),
            r"c:\users\example\downloads"
        );
        assert_eq!(
            normalize_folder_path(r"\\?\C:\Users\Example\Downloads\"),
            r"c:\users\example\downloads"
        );
        assert_eq!(
            normalize_folder_path(r"\\?\UNC\Server\Share\Reports"),
            r"\\server\share\reports"
        );
    }

    #[test]
    fn loading_missing_empty_or_malformed_file_returns_no_favorites() {
        let dir = test_dir("empty");
        let missing = dir.join("missing.json");
        assert!(load_favorite_folders_from_file(&missing).is_empty());

        let empty = dir.join("empty.json");
        fs::write(&empty, "").expect("empty file should be written");
        assert!(load_favorite_folders_from_file(&empty).is_empty());

        let malformed = dir.join("malformed.json");
        fs::write(&malformed, "not json").expect("malformed file should be written");
        assert!(load_favorite_folders_from_file(&malformed).is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn loading_valid_file_collapses_duplicate_paths() {
        let dir = test_dir("duplicates");
        let folder = dir.join("Downloads");
        fs::create_dir_all(&folder).expect("favorite folder should be created");
        let file = dir.join("favorites.json");
        let json = serde_json::to_string(&vec![
            StoredFavoriteFolder {
                path: folder.to_string_lossy().to_string(),
            },
            StoredFavoriteFolder {
                path: folder.to_string_lossy().to_string(),
            },
        ])
        .expect("favorites should serialize");
        fs::write(&file, json).expect("favorites file should be written");

        let loaded = load_favorite_folders_from_file(&file);

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Downloads");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn save_and_load_favorite_folders_round_trips_valid_folders() {
        let dir = test_dir("roundtrip");
        let folder = dir.join("Projects");
        fs::create_dir_all(&folder).expect("favorite folder should be created");
        let file = dir.join("favorites.json");
        let entry = favorite_folder_from_path(&folder).expect("folder should become favorite");

        save_favorite_folders_to_file(&file, &[entry]).expect("favorites should save");
        let loaded = load_favorite_folders_from_file(&file);

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Projects");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn add_and_remove_favorite_folder_paths_update_collection() {
        let dir = test_dir("add-remove");
        let folder = dir.join("Projects");
        fs::create_dir_all(&folder).expect("favorite folder should be created");
        let mut folders = Vec::new();

        let first = add_favorite_folder_path(&mut folders, &folder)
            .expect("folder should be added as favorite");
        let duplicate = add_favorite_folder_path(&mut folders, &folder)
            .expect("duplicate folder should still resolve");

        assert!(first.added);
        assert!(!duplicate.added);
        assert_eq!(folders.len(), 1);
        assert!(remove_favorite_folder_path(&mut folders, &first.entry.path));
        assert!(folders.is_empty());

        let _ = fs::remove_dir_all(dir);
    }
}
