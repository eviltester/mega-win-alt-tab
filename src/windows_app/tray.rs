use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_SETVERSION, NIN_SELECT, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, PostMessageW, SetForegroundWindow,
    TrackPopupMenu, HICON, MENU_ITEM_FLAGS, MF_CHECKED, MF_SEPARATOR, MF_STRING, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, WM_APP, WM_CONTEXTMENU, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP,
};

pub(super) const TRAY_ICON_ID: u32 = 1;
pub(super) const WM_TRAYICON: u32 = WM_APP + 1;
const TRAY_STARTUP_COMMAND_ID: usize = 1000;
const TRAY_EXIT_COMMAND_ID: usize = 1001;
static MENU_OPEN: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TrayMenuCommand {
    None,
    ToggleStartup,
    Exit,
}

pub(super) unsafe fn show_context_menu(hwnd: HWND, run_at_startup: bool) -> TrayMenuCommand {
    if MENU_OPEN
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return TrayMenuCommand::None;
    }

    let command = show_context_menu_inner(hwnd, run_at_startup);
    MENU_OPEN.store(false, Ordering::Release);
    command
}

unsafe fn show_context_menu_inner(hwnd: HWND, run_at_startup: bool) -> TrayMenuCommand {
    let Ok(menu) = CreatePopupMenu() else {
        return TrayMenuCommand::None;
    };
    let startup_flags = checked_menu_flags(run_at_startup);
    let _ = AppendMenuW(
        menu,
        startup_flags,
        TRAY_STARTUP_COMMAND_ID,
        w!("Run at startup"),
    );
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    let _ = AppendMenuW(menu, MF_STRING, TRAY_EXIT_COMMAND_ID, w!("Exit"));

    let mut point = POINT::default();
    let mut selected = TrayMenuCommand::None;
    if GetCursorPos(&mut point).is_ok() {
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

fn menu_command_from_id(command_id: usize) -> TrayMenuCommand {
    match command_id {
        TRAY_STARTUP_COMMAND_ID => TrayMenuCommand::ToggleStartup,
        TRAY_EXIT_COMMAND_ID => TrayMenuCommand::Exit,
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
        NIN_SELECT | WM_LBUTTONUP | WM_LBUTTONDBLCLK
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
}
