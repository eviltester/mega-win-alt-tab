use super::settings::ActivationHotkey;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_SELECT, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, PostMessageW, SetForegroundWindow,
    TrackPopupMenu, HICON, MENU_ITEM_FLAGS, MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_CONTEXTMENU, WM_LBUTTONDBLCLK, WM_LBUTTONUP,
    WM_NULL, WM_RBUTTONUP,
};

pub(super) const TRAY_ICON_ID: u32 = 1;
pub(super) const WM_TRAYICON: u32 = WM_APP + 1;
const NIN_KEYSELECT_EVENT: u32 = NIN_SELECT | 1;
const TRAY_OPEN_COMMAND_ID: usize = 999;
const TRAY_STARTUP_COMMAND_ID: usize = 1000;
const TRAY_EXIT_COMMAND_ID: usize = 1001;
const TRAY_OPEN_LOG_COMMAND_ID: usize = 1002;
const TRAY_COPY_LOG_PATH_COMMAND_ID: usize = 1003;
const TRAY_HOTKEY_COMMAND_BASE_ID: usize = 1100;
const TRAY_ACTIVE_CUSTOM_HOTKEY_COMMAND_ID: usize = 1200;
const TRAY_CHANGE_HOTKEY_COMMAND_ID: usize = 1201;
static MENU_OPEN: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TrayMenuCommand {
    None,
    Open,
    OpenLogFile,
    CopyLogFilePath,
    SetActivationHotkey(ActivationHotkey),
    CaptureActivationHotkey,
    ToggleStartup,
    Exit,
}

pub(super) unsafe fn show_context_menu(
    hwnd: HWND,
    run_at_startup: bool,
    activation_hotkey: ActivationHotkey,
) -> TrayMenuCommand {
    show_context_menu_at(hwnd, run_at_startup, activation_hotkey, None)
}

pub(super) unsafe fn show_context_menu_at(
    hwnd: HWND,
    run_at_startup: bool,
    activation_hotkey: ActivationHotkey,
    position: Option<POINT>,
) -> TrayMenuCommand {
    if MENU_OPEN
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return TrayMenuCommand::None;
    }

    let command = show_context_menu_inner(hwnd, run_at_startup, activation_hotkey, position);
    MENU_OPEN.store(false, Ordering::Release);
    command
}

unsafe fn show_context_menu_inner(
    hwnd: HWND,
    run_at_startup: bool,
    activation_hotkey: ActivationHotkey,
    position: Option<POINT>,
) -> TrayMenuCommand {
    let Ok(menu) = CreatePopupMenu() else {
        return TrayMenuCommand::None;
    };
    let _ = AppendMenuW(menu, MF_STRING, TRAY_OPEN_COMMAND_ID, w!("Open switcher"));

    if let Ok(configure_menu) = CreatePopupMenu() {
        for (index, hotkey) in ActivationHotkey::ALL.iter().copied().enumerate() {
            let flags = checked_menu_flags(hotkey == activation_hotkey);
            let label = hotkey.label();
            append_menu_text(configure_menu, hotkey_command_id(index), &label, flags);
        }
        if let Some(label) = active_custom_hotkey_label(activation_hotkey) {
            append_menu_text(
                configure_menu,
                TRAY_ACTIVE_CUSTOM_HOTKEY_COMMAND_ID,
                &label,
                checked_menu_flags(true),
            );
        }
        let _ = AppendMenuW(configure_menu, MF_SEPARATOR, 0, None);
        append_menu_text(
            configure_menu,
            TRAY_CHANGE_HOTKEY_COMMAND_ID,
            "Change Shortcut Keys...",
            MF_STRING,
        );

        let configure_flags = MENU_ITEM_FLAGS(MF_POPUP.0 | MF_STRING.0);
        let configure_label = to_wide_null("Configure");
        if AppendMenuW(
            menu,
            configure_flags,
            configure_menu.0 as usize,
            PCWSTR(configure_label.as_ptr()),
        )
        .is_err()
        {
            let _ = DestroyMenu(configure_menu);
        }
    }

    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    let _ = AppendMenuW(
        menu,
        MF_STRING,
        TRAY_OPEN_LOG_COMMAND_ID,
        w!("Open app log file"),
    );
    let _ = AppendMenuW(
        menu,
        MF_STRING,
        TRAY_COPY_LOG_PATH_COMMAND_ID,
        w!("Copy log file path"),
    );
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    let startup_flags = checked_menu_flags(run_at_startup);
    let _ = AppendMenuW(
        menu,
        startup_flags,
        TRAY_STARTUP_COMMAND_ID,
        w!("Run at startup"),
    );
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    let _ = AppendMenuW(menu, MF_STRING, TRAY_EXIT_COMMAND_ID, w!("Exit"));

    let mut point = position.unwrap_or_default();
    let mut selected = TrayMenuCommand::None;
    if position.is_some() || GetCursorPos(&mut point).is_ok() {
        let _ = SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD,
            point.x,
            point.y,
            0,
            hwnd,
            None,
        );
        selected = menu_command_from_id(command.0 as usize);
        let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
    }

    let _ = DestroyMenu(menu);
    selected
}

fn checked_menu_flags(checked: bool) -> MENU_ITEM_FLAGS {
    if checked {
        MENU_ITEM_FLAGS(MF_STRING.0 | MF_CHECKED.0)
    } else {
        MF_STRING
    }
}

fn active_custom_hotkey_label(hotkey: ActivationHotkey) -> Option<String> {
    matches!(hotkey, ActivationHotkey::Custom { .. }).then(|| format!("Custom: {}", hotkey.label()))
}

unsafe fn append_menu_text(
    menu: windows::Win32::UI::WindowsAndMessaging::HMENU,
    id: usize,
    text: &str,
    flags: MENU_ITEM_FLAGS,
) {
    let text = to_wide_null(text);
    let _ = AppendMenuW(menu, flags, id, PCWSTR(text.as_ptr()));
}

fn hotkey_command_id(index: usize) -> usize {
    TRAY_HOTKEY_COMMAND_BASE_ID + index
}

fn menu_command_from_id(command_id: usize) -> TrayMenuCommand {
    match command_id {
        TRAY_OPEN_COMMAND_ID => TrayMenuCommand::Open,
        TRAY_OPEN_LOG_COMMAND_ID => TrayMenuCommand::OpenLogFile,
        TRAY_COPY_LOG_PATH_COMMAND_ID => TrayMenuCommand::CopyLogFilePath,
        TRAY_CHANGE_HOTKEY_COMMAND_ID => TrayMenuCommand::CaptureActivationHotkey,
        TRAY_ACTIVE_CUSTOM_HOTKEY_COMMAND_ID => TrayMenuCommand::None,
        TRAY_STARTUP_COMMAND_ID => TrayMenuCommand::ToggleStartup,
        TRAY_EXIT_COMMAND_ID => TrayMenuCommand::Exit,
        command_id
            if (TRAY_HOTKEY_COMMAND_BASE_ID
                ..TRAY_HOTKEY_COMMAND_BASE_ID + ActivationHotkey::ALL.len())
                .contains(&command_id) =>
        {
            TrayMenuCommand::SetActivationHotkey(
                ActivationHotkey::ALL[command_id - TRAY_HOTKEY_COMMAND_BASE_ID],
            )
        }
        _ => TrayMenuCommand::None,
    }
}

pub(super) fn is_tray_icon_message(wparam: WPARAM, lparam: LPARAM) -> bool {
    let legacy_icon_id = (wparam.0 & 0xffff) as u32;
    let v4_icon_id = ((lparam.0 as usize >> 16) & 0xffff) as u32;
    legacy_icon_id == TRAY_ICON_ID || v4_icon_id == TRAY_ICON_ID
}

pub(super) fn is_tray_context_event(lparam: LPARAM) -> bool {
    matches!(lparam_low_word(lparam), WM_RBUTTONUP | WM_CONTEXTMENU)
}

pub(super) fn is_tray_select_event(lparam: LPARAM) -> bool {
    matches!(
        lparam_low_word(lparam),
        NIN_SELECT | NIN_KEYSELECT_EVENT | WM_LBUTTONUP | WM_LBUTTONDBLCLK
    )
}

fn lparam_low_word(lparam: LPARAM) -> u32 {
    (lparam.0 as usize & 0xffff) as u32
}

pub(super) unsafe fn install_tray_icon(hwnd: HWND, icon: HICON) -> bool {
    let mut data = tray_icon_data(hwnd);
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    data.uCallbackMessage = WM_TRAYICON;
    data.hIcon = icon;
    write_wide_fixed(&mut data.szTip, "Mega Win Alt Tab");

    if !Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
        return false;
    }
    data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
    true
}

pub(super) unsafe fn remove_tray_icon(hwnd: HWND) {
    let data = tray_icon_data(hwnd);
    let _ = Shell_NotifyIconW(NIM_DELETE, &data);
}

pub(super) unsafe fn show_update_notification(hwnd: HWND, latest_version: &str) {
    let mut data = tray_icon_data(hwnd);
    data.uFlags = NIF_INFO;
    data.dwInfoFlags = NIIF_INFO;
    write_wide_fixed(&mut data.szInfoTitle, "Mega Win Alt Tab update");
    write_wide_fixed(&mut data.szInfo, &update_notification_text(latest_version));
    let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
}

pub(super) unsafe fn show_hotkey_unavailable_notification(hwnd: HWND, hotkey_label: &str) {
    let mut data = tray_icon_data(hwnd);
    data.uFlags = NIF_INFO;
    data.dwInfoFlags = NIIF_INFO;
    write_wide_fixed(&mut data.szInfoTitle, "Mega Win Alt Tab hotkey unavailable");
    write_wide_fixed(
        &mut data.szInfo,
        &format!(
            "{hotkey_label} is in use by another app. Click the tray icon or right-click and choose Open switcher."
        ),
    );
    let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
}

pub(super) unsafe fn show_log_path_copied_notification(hwnd: HWND) {
    let mut data = tray_icon_data(hwnd);
    data.uFlags = NIF_INFO;
    data.dwInfoFlags = NIIF_INFO;
    write_wide_fixed(&mut data.szInfoTitle, "Mega Win Alt Tab");
    write_wide_fixed(
        &mut data.szInfo,
        "App log file path copied to the clipboard.",
    );
    let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
}

fn update_notification_text(latest_version: &str) -> String {
    format!("Version {latest_version} is available on GitHub Releases.")
}

fn tray_icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ICON_ID,
        ..Default::default()
    }
}

fn write_wide_fixed<const N: usize>(target: &mut [u16; N], value: &str) {
    for (slot, ch) in target.iter_mut().zip(value.encode_utf16().take(N - 1)) {
        *slot = ch;
    }
}

fn to_wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tray_lparam(event: u32, icon_id: u32) -> LPARAM {
        LPARAM((((icon_id as usize) << 16) | (event as usize & 0xffff)) as isize)
    }

    #[test]
    fn tray_icon_message_accepts_legacy_and_v4_icon_ids() {
        assert!(is_tray_icon_message(
            WPARAM(TRAY_ICON_ID as usize),
            LPARAM(WM_RBUTTONUP as isize)
        ));
        assert!(is_tray_icon_message(
            WPARAM(0),
            tray_lparam(NIN_SELECT, TRAY_ICON_ID)
        ));
        assert!(!is_tray_icon_message(
            WPARAM(0),
            tray_lparam(NIN_SELECT, TRAY_ICON_ID + 1)
        ));
        assert!(!is_tray_icon_message(
            WPARAM((TRAY_ICON_ID + 1) as usize),
            tray_lparam(NIN_SELECT, TRAY_ICON_ID + 1)
        ));
    }

    #[test]
    fn tray_event_classifiers_split_select_from_context_menu() {
        assert!(is_tray_select_event(tray_lparam(NIN_SELECT, TRAY_ICON_ID)));
        assert!(is_tray_select_event(tray_lparam(
            NIN_KEYSELECT_EVENT,
            TRAY_ICON_ID
        )));
        assert!(is_tray_select_event(LPARAM(WM_LBUTTONUP as isize)));
        assert!(is_tray_select_event(LPARAM(WM_LBUTTONDBLCLK as isize)));
        assert!(!is_tray_select_event(LPARAM(WM_RBUTTONUP as isize)));

        assert!(is_tray_context_event(LPARAM(WM_RBUTTONUP as isize)));
        assert!(is_tray_context_event(LPARAM(WM_CONTEXTMENU as isize)));
        assert!(!is_tray_context_event(tray_lparam(
            NIN_SELECT,
            TRAY_ICON_ID
        )));
    }

    #[test]
    fn menu_command_ids_map_to_actions() {
        assert_eq!(
            menu_command_from_id(TRAY_OPEN_COMMAND_ID),
            TrayMenuCommand::Open
        );
        assert_eq!(
            menu_command_from_id(TRAY_OPEN_LOG_COMMAND_ID),
            TrayMenuCommand::OpenLogFile
        );
        assert_eq!(
            menu_command_from_id(TRAY_COPY_LOG_PATH_COMMAND_ID),
            TrayMenuCommand::CopyLogFilePath
        );
        assert_eq!(
            menu_command_from_id(TRAY_CHANGE_HOTKEY_COMMAND_ID),
            TrayMenuCommand::CaptureActivationHotkey
        );
        assert_eq!(
            menu_command_from_id(TRAY_ACTIVE_CUSTOM_HOTKEY_COMMAND_ID),
            TrayMenuCommand::None
        );
        for (index, hotkey) in ActivationHotkey::ALL.iter().copied().enumerate() {
            assert_eq!(
                menu_command_from_id(hotkey_command_id(index)),
                TrayMenuCommand::SetActivationHotkey(hotkey)
            );
        }
        assert_eq!(
            menu_command_from_id(TRAY_STARTUP_COMMAND_ID),
            TrayMenuCommand::ToggleStartup
        );
        assert_eq!(
            menu_command_from_id(TRAY_EXIT_COMMAND_ID),
            TrayMenuCommand::Exit
        );
        assert_eq!(menu_command_from_id(0), TrayMenuCommand::None);
    }

    #[test]
    fn startup_menu_item_can_be_checked() {
        assert_eq!(checked_menu_flags(false), MF_STRING);
        assert_eq!(
            checked_menu_flags(true),
            MENU_ITEM_FLAGS(MF_STRING.0 | MF_CHECKED.0)
        );
    }

    #[test]
    fn active_custom_hotkey_is_named_in_the_configure_menu() {
        let custom = ActivationHotkey::custom(true, true, false, b'P' as u32).unwrap();

        assert_eq!(
            active_custom_hotkey_label(custom),
            Some("Custom: Ctrl+Alt+P".to_owned())
        );
        assert_eq!(
            active_custom_hotkey_label(ActivationHotkey::CtrlAltSpace),
            None
        );
    }

    #[test]
    fn update_notification_names_latest_version() {
        assert_eq!(
            update_notification_text("v1.2.3"),
            "Version v1.2.3 is available on GitHub Releases."
        );
    }
}
