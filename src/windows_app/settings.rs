use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_SHIFT, VK_BACK, VK_DELETE, VK_DOWN, VK_END,
    VK_ESCAPE, VK_F1, VK_F12, VK_F24, VK_HOME, VK_INSERT, VK_LEFT, VK_NEXT, VK_OEM_1, VK_OEM_2,
    VK_OEM_3, VK_OEM_4, VK_OEM_5, VK_OEM_6, VK_OEM_7, VK_OEM_COMMA, VK_OEM_MINUS, VK_OEM_PERIOD,
    VK_OEM_PLUS, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
};

const SETTINGS_FILE_NAME: &str = "settings.json";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ActivationHotkey {
    #[default]
    CtrlAltSpace,
    CtrlShiftSpace,
    AltShiftSpace,
    CtrlAltF12,
    CtrlShiftF12,
    Custom {
        ctrl: bool,
        alt: bool,
        shift: bool,
        virtual_key: u32,
    },
}

impl ActivationHotkey {
    pub(super) const ALL: [Self; 5] = [
        Self::CtrlAltSpace,
        Self::CtrlShiftSpace,
        Self::AltShiftSpace,
        Self::CtrlAltF12,
        Self::CtrlShiftF12,
    ];

    pub(super) fn custom(ctrl: bool, alt: bool, shift: bool, virtual_key: u32) -> Option<Self> {
        if (!ctrl && !alt && !shift)
            || is_modifier_key(virtual_key)
            || !(1..=0xfe).contains(&virtual_key)
        {
            return None;
        }

        Some(
            Self::Custom {
                ctrl,
                alt,
                shift,
                virtual_key,
            }
            .canonical(),
        )
    }

    pub(super) fn label(self) -> String {
        match self {
            Self::CtrlAltSpace => "Ctrl+Alt+Space".to_string(),
            Self::CtrlShiftSpace => "Ctrl+Shift+Space".to_string(),
            Self::AltShiftSpace => "Alt+Shift+Space".to_string(),
            Self::CtrlAltF12 => "Ctrl+Alt+F12".to_string(),
            Self::CtrlShiftF12 => "Ctrl+Shift+F12".to_string(),
            Self::Custom {
                ctrl,
                alt,
                shift,
                virtual_key,
            } => {
                let mut parts = Vec::with_capacity(4);
                if ctrl {
                    parts.push("Ctrl".to_string());
                }
                if alt {
                    parts.push("Alt".to_string());
                }
                if shift {
                    parts.push("Shift".to_string());
                }
                parts.push(virtual_key_label(virtual_key));
                parts.join("+")
            }
        }
    }

    pub(super) fn modifiers(self) -> HOT_KEY_MODIFIERS {
        match self {
            Self::CtrlAltSpace | Self::CtrlAltF12 => MOD_CONTROL | MOD_ALT,
            Self::CtrlShiftSpace | Self::CtrlShiftF12 => MOD_CONTROL | MOD_SHIFT,
            Self::AltShiftSpace => MOD_ALT | MOD_SHIFT,
            Self::Custom {
                ctrl, alt, shift, ..
            } => {
                let mut modifiers = HOT_KEY_MODIFIERS(0);
                if ctrl {
                    modifiers |= MOD_CONTROL;
                }
                if alt {
                    modifiers |= MOD_ALT;
                }
                if shift {
                    modifiers |= MOD_SHIFT;
                }
                modifiers
            }
        }
    }

    pub(super) fn virtual_key(self) -> u32 {
        match self {
            Self::CtrlAltSpace | Self::CtrlShiftSpace | Self::AltShiftSpace => VK_SPACE.0 as u32,
            Self::CtrlAltF12 | Self::CtrlShiftF12 => VK_F12.0 as u32,
            Self::Custom { virtual_key, .. } => virtual_key,
        }
    }

    fn canonical(self) -> Self {
        Self::ALL
            .iter()
            .copied()
            .find(|preset| {
                preset.modifiers() == self.modifiers() && preset.virtual_key() == self.virtual_key()
            })
            .unwrap_or(self)
    }

    fn validated(self) -> Self {
        match self {
            Self::Custom {
                ctrl,
                alt,
                shift,
                virtual_key,
            } => Self::custom(ctrl, alt, shift, virtual_key).unwrap_or_default(),
            preset => preset,
        }
    }
}

pub(super) fn is_modifier_key(virtual_key: u32) -> bool {
    matches!(
        virtual_key,
        0x10 | 0x11 | 0x12 | 0x5b | 0x5c | 0xa0 | 0xa1 | 0xa2 | 0xa3 | 0xa4 | 0xa5
    )
}

fn virtual_key_label(virtual_key: u32) -> String {
    if (b'A' as u32..=b'Z' as u32).contains(&virtual_key)
        || (b'0' as u32..=b'9' as u32).contains(&virtual_key)
    {
        return char::from_u32(virtual_key)
            .map(|key| key.to_string())
            .unwrap_or_else(|| format!("Key 0x{virtual_key:02X}"));
    }
    if (VK_F1.0 as u32..=VK_F24.0 as u32).contains(&virtual_key) {
        return format!("F{}", virtual_key - VK_F1.0 as u32 + 1);
    }

    let label = match virtual_key {
        key if key == VK_BACK.0 as u32 => "Backspace",
        key if key == VK_TAB.0 as u32 => "Tab",
        key if key == VK_RETURN.0 as u32 => "Enter",
        key if key == VK_ESCAPE.0 as u32 => "Esc",
        key if key == VK_SPACE.0 as u32 => "Space",
        key if key == VK_PRIOR.0 as u32 => "Page Up",
        key if key == VK_NEXT.0 as u32 => "Page Down",
        key if key == VK_END.0 as u32 => "End",
        key if key == VK_HOME.0 as u32 => "Home",
        key if key == VK_LEFT.0 as u32 => "Left",
        key if key == VK_UP.0 as u32 => "Up",
        key if key == VK_RIGHT.0 as u32 => "Right",
        key if key == VK_DOWN.0 as u32 => "Down",
        key if key == VK_INSERT.0 as u32 => "Insert",
        key if key == VK_DELETE.0 as u32 => "Delete",
        key if key == VK_OEM_1.0 as u32 => ";",
        key if key == VK_OEM_PLUS.0 as u32 => "=",
        key if key == VK_OEM_COMMA.0 as u32 => ",",
        key if key == VK_OEM_MINUS.0 as u32 => "-",
        key if key == VK_OEM_PERIOD.0 as u32 => ".",
        key if key == VK_OEM_2.0 as u32 => "/",
        key if key == VK_OEM_3.0 as u32 => "`",
        key if key == VK_OEM_4.0 as u32 => "[",
        key if key == VK_OEM_5.0 as u32 => "\\",
        key if key == VK_OEM_6.0 as u32 => "]",
        key if key == VK_OEM_7.0 as u32 => "'",
        _ => return format!("Key 0x{virtual_key:02X}"),
    };
    label.to_string()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct UserSettings {
    #[serde(default)]
    pub(super) activation_hotkey: ActivationHotkey,
}

pub(super) fn load_user_settings() -> UserSettings {
    load_user_settings_from(&settings_file_path())
}

pub(super) fn save_user_settings(settings: UserSettings) -> bool {
    save_user_settings_to(&settings_file_path(), settings)
}

pub(super) fn settings_file_path() -> PathBuf {
    settings_dir().join(SETTINGS_FILE_NAME)
}

pub(super) fn settings_dir() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("MegaWinAltTab")
}

fn load_user_settings_from(path: &Path) -> UserSettings {
    let Ok(json) = fs::read_to_string(path) else {
        return UserSettings::default();
    };

    let mut settings: UserSettings = serde_json::from_str(&json).unwrap_or_default();
    settings.activation_hotkey = settings.activation_hotkey.validated();
    settings
}

fn save_user_settings_to(path: &Path, settings: UserSettings) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    if fs::create_dir_all(parent).is_err() {
        return false;
    }
    let Ok(json) = serde_json::to_string_pretty(&settings) else {
        return false;
    };

    fs::write(path, format!("{json}\n")).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after Unix epoch")
            .as_nanos();
        env::temp_dir().join(format!(
            "mega-win-alt-tab-settings-{name}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn missing_or_malformed_settings_use_default_hotkey() {
        let root = test_dir("defaults");
        let path = root.join(SETTINGS_FILE_NAME);

        assert_eq!(load_user_settings_from(&path), UserSettings::default());
        fs::create_dir_all(&root).expect("test directory should be created");
        fs::write(&path, "not json").expect("malformed settings should be written");
        assert_eq!(load_user_settings_from(&path), UserSettings::default());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn settings_round_trip_selected_activation_hotkey() {
        let root = test_dir("round-trip");
        let path = root.join(SETTINGS_FILE_NAME);
        let settings = UserSettings {
            activation_hotkey: ActivationHotkey::CtrlShiftF12,
        };

        assert!(save_user_settings_to(&path, settings));
        assert_eq!(load_user_settings_from(&path), settings);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn settings_round_trip_custom_activation_hotkey() {
        let root = test_dir("custom-round-trip");
        let path = root.join(SETTINGS_FILE_NAME);
        let settings = UserSettings {
            activation_hotkey: ActivationHotkey::custom(true, false, true, b'K' as u32)
                .expect("Ctrl+Shift+K should be valid"),
        };

        assert!(save_user_settings_to(&path, settings));
        assert_eq!(load_user_settings_from(&path), settings);
        let json = fs::read_to_string(&path).expect("settings should be readable");
        assert!(json.contains("\"custom\""));
        assert!(json.contains("\"virtual_key\": 75"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_custom_settings_fall_back_to_default() {
        let root = test_dir("invalid-custom");
        let path = root.join(SETTINGS_FILE_NAME);
        fs::create_dir_all(&root).expect("test directory should be created");
        fs::write(
            &path,
            r#"{"activation_hotkey":{"custom":{"ctrl":false,"alt":false,"shift":false,"virtual_key":75}}}"#,
        )
        .expect("invalid custom settings should be written");

        assert_eq!(load_user_settings_from(&path), UserSettings::default());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn activation_hotkeys_have_unique_labels_and_bindings() {
        let mut labels = ActivationHotkey::ALL
            .iter()
            .map(|hotkey| hotkey.label())
            .collect::<Vec<_>>();
        labels.sort_unstable();
        labels.dedup();

        let mut bindings = ActivationHotkey::ALL
            .iter()
            .map(|hotkey| (hotkey.modifiers().0, hotkey.virtual_key()))
            .collect::<Vec<_>>();
        bindings.sort_unstable();
        bindings.dedup();

        assert_eq!(labels.len(), ActivationHotkey::ALL.len());
        assert_eq!(bindings.len(), ActivationHotkey::ALL.len());
    }

    #[test]
    fn custom_shortcut_round_trips_with_human_readable_label() {
        let hotkey = ActivationHotkey::custom(true, true, false, b'K' as u32)
            .expect("Ctrl+Alt+K should be valid");

        assert_eq!(hotkey.label(), "Ctrl+Alt+K");
        assert_eq!(hotkey.modifiers(), MOD_CONTROL | MOD_ALT);
        assert_eq!(hotkey.virtual_key(), b'K' as u32);
    }

    #[test]
    fn custom_shortcut_requires_modifier_and_non_modifier_key() {
        assert_eq!(
            ActivationHotkey::custom(false, false, false, b'K' as u32),
            None
        );
        assert_eq!(ActivationHotkey::custom(true, false, false, 0x11), None);
    }

    #[test]
    fn custom_shortcut_canonicalizes_a_matching_preset() {
        assert_eq!(
            ActivationHotkey::custom(true, true, false, VK_SPACE.0 as u32),
            Some(ActivationHotkey::CtrlAltSpace)
        );
    }
}
