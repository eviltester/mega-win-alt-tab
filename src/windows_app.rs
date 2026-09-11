use mega_win_alt_tab::core::{
    build_launcher_results, build_results_with_options, normalize_for_match, ActivationTarget,
    AppEntry, BuildResultOptions, FavoriteFolderEntry, SearchResult, SearchResultKind, TabEntry,
    TabSource, WindowEntry,
};
use mega_win_alt_tab::extension_bridge::ExtensionBridge;
mod apps;
mod folders;
mod icons;
mod input;
mod monitors;
mod startup;
mod tray;
mod updates;
mod virtual_desktops;

use apps::{enumerate_apps, launch_app};
use folders::{
    add_favorite_folder_path, dropped_file_paths, load_favorite_folders, normalize_folder_path,
    open_folder, remove_favorite_folder_path, save_favorite_folders,
};
use icons::create_mega_icon;
use input::{mouse_point, point_in_rect};
use monitors::{
    current_window_rect, enumerate_monitor_numbers, monitor_rect_for_window,
    move_window_to_monitor, window_screen_number, MonitorMoveDirection,
};
use startup::{
    is_run_at_startup_enabled, legacy_startup_entries, remove_startup_entries, set_run_at_startup,
    LegacyStartupEntry,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::{c_void, OsStr};
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::mem::size_of;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::{self, sleep};
use std::time::{Duration, Instant, SystemTime};
use tray::{
    install_tray_icon, is_tray_context_event, is_tray_icon_message, is_tray_select_event,
    remove_tray_icon, show_context_menu, show_update_notification, TrayMenuCommand, WM_TRAYICON,
};
use updates::{check_for_update, UpdateInfo};
use virtual_desktops::{
    move_window_to_overlay_desktop_if_needed, should_include_window_for_desktop,
    virtual_desktop_manager, window_desktop_location,
};
use windows::core::{w, Result, PCWSTR, PWSTR, VARIANT};
use windows::Win32::Foundation::{
    CloseHandle, BOOL, COLORREF, HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DwmRegisterThumbnail, DwmUnregisterThumbnail,
    DwmUpdateThumbnailProperties, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_VISIBLE,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreatePen, CreateSolidBrush, DeleteObject, DrawTextW, Ellipse,
    EndPaint, FillRect, InvalidateRect, LineTo, MoveToEx, SelectObject, SetBkMode, SetTextColor,
    DRAW_TEXT_FORMAT, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE,
    DT_VCENTER, FW_BOLD, FW_NORMAL, HBRUSH, HDC, HFONT, PAINTSTRUCT, PS_SOLID, TRANSPARENT,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationCondition, IUIAutomationElement,
    IUIAutomationInvokePattern, IUIAutomationSelectionItemPattern, TreeScope_Descendants,
    UIA_ControlTypePropertyId, UIA_InvokePatternId, UIA_SelectionItemPatternId,
    UIA_TabItemControlTypeId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, RegisterHotKey, SetFocus, TrackMouseEvent, UnregisterHotKey, MOD_ALT, MOD_CONTROL,
    MOD_NOREPEAT, TME_LEAVE, TRACKMOUSEEVENT, VK_BACK, VK_CONTROL, VK_D, VK_DOWN, VK_ESCAPE,
    VK_LEFT, VK_OEM_2, VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::Shell::{DragAcceptFiles, IVirtualDesktopManager, ShellExecuteW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, DispatchMessageW, DrawIconEx,
    EnumWindows, GetClassNameW, GetClientRect, GetMessageW, GetShellWindow, GetSystemMetrics,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindow, IsWindowVisible, IsZoomed, KillTimer, LoadCursorW, LoadIconW, MessageBoxW,
    PostMessageW, PostQuitMessage, RegisterClassW, SendMessageW, SetForegroundWindow, SetTimer,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW,
    DI_NORMAL, GWLP_USERDATA, GWL_EXSTYLE, HICON, HTTRANSPARENT, HWND_TOP, HWND_TOPMOST, ICON_BIG,
    ICON_SMALL, IDC_ARROW, IDI_APPLICATION, IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO, MSG,
    SET_WINDOW_POS_FLAGS, SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SW_HIDE, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE, SW_SHOWNORMAL,
    WM_CHAR, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_DROPFILES, WM_HOTKEY, WM_KEYDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE, WM_NCHITTEST, WM_PAINT, WM_RBUTTONUP, WM_SETICON,
    WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP,
};

const HOTKEY_ID: i32 = 0x4d57;
const SPLASH_TIMER_ID: usize = 0x4d58;
const CLOSE_REFRESH_TIMER_ID: usize = 0x4d59;
const APP_SCAN_TIMER_ID: usize = 0x4d5a;
const HIGHLIGHT_TIMER_ID: usize = 0x4d5b;
const UPDATE_CHECK_TIMER_ID: usize = 0x4d5c;
const SPLASH_DURATION_MS: u32 = 2400;
const CLOSE_REFRESH_POLL_MS: u32 = 250;
const CLOSE_REFRESH_ATTEMPTS: u8 = 20;
const APP_SCAN_POLL_MS: u32 = 100;
const UPDATE_CHECK_POLL_MS: u32 = 250;
const HIGHLIGHT_DURATION_MS: u32 = 900;
const HIGHLIGHT_SECOND_TAP_MS: u64 = 1600;
const HIGHLIGHT_BORDER_PX: i32 = 6;
const WM_MOUSELEAVE_MESSAGE: u32 = 0x02A3;
const MAX_RESULTS: usize = 8;
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const GITHUB_RELEASES_URL: PCWSTR = w!("https://github.com/eviltester/mega-win-alt-tab/releases");
const KEY_F: u32 = b'F' as u32;
const KEY_S: u32 = b'S' as u32;
const KEY_W: u32 = b'W' as u32;
const HELP_LINES: [&str; 12] = [
    concat!("Version ", env!("CARGO_PKG_VERSION")),
    "Esc - close",
    "Up / Down - move selection",
    "Enter - select or launch",
    "Left / Right - show without focus; second tap highlights",
    "Ctrl + F - maximize or restore selected window",
    "Ctrl + S - minimize selected window",
    "Ctrl + W - close selected window or remove favorite folder",
    "Ctrl + Left - move selected window to previous screen in layout",
    "Ctrl + Right - move selected window to next screen in layout",
    "Ctrl + D - search all desktops",
    "Ctrl + ? / Ctrl + / - app launcher mode",
];
const THUMBNAIL_SIZES: [ThumbnailSize; 4] = [
    ThumbnailSize {
        width: 112,
        height: 52,
    },
    ThumbnailSize {
        width: 168,
        height: 95,
    },
    ThumbnailSize {
        width: 240,
        height: 135,
    },
    ThumbnailSize {
        width: 320,
        height: 180,
    },
];
const CLASS_NAME: PCWSTR = w!("MegaWinAltTabOverlay");
const HIGHLIGHT_CLASS_NAME: PCWSTR = w!("MegaWinAltTabHighlight");
const WINDOW_TITLE: PCWSTR = w!("Mega Win Alt Tab");

pub fn run() -> Result<()> {
    let startup_launch = has_startup_arg(env::args_os());

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?;
        let cursor = LoadCursorW(None, IDC_ARROW)?;
        let (icon, owns_icon) = match create_mega_icon() {
            Some(icon) => (icon, true),
            None => (LoadIconW(None, IDI_APPLICATION)?, false),
        };
        let class = WNDCLASSW {
            hCursor: cursor,
            hIcon: icon,
            hInstance: instance.into(),
            lpszClassName: CLASS_NAME,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            ..Default::default()
        };
        let _ = RegisterClassW(&class);
        let highlight_class = WNDCLASSW {
            hCursor: cursor,
            hInstance: instance.into(),
            lpszClassName: HIGHLIGHT_CLASS_NAME,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(highlight_wnd_proc),
            ..Default::default()
        };
        let _ = RegisterClassW(&highlight_class);

        let bridge = ExtensionBridge::start().map_err(to_win_error)?;
        let state = Rc::new(RefCell::new(AppState::new(bridge, icon, owns_icon)));
        let raw_state = Rc::into_raw(state);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            CLASS_NAME,
            WINDOW_TITLE,
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            instance,
            Some(raw_state as *const c_void),
        )?;
        DragAcceptFiles(hwnd, true);
        let _ = SendMessageW(
            hwnd,
            WM_SETICON,
            WPARAM(ICON_SMALL as usize),
            LPARAM(icon.0 as isize),
        );
        let _ = SendMessageW(
            hwnd,
            WM_SETICON,
            WPARAM(ICON_BIG as usize),
            LPARAM(icon.0 as isize),
        );

        (*raw_state).borrow_mut().install_tray_icon();

        RegisterHotKey(
            hwnd,
            HOTKEY_ID,
            MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
            VK_SPACE.0 as u32,
        )?;

        if !startup_launch {
            (*raw_state).borrow_mut().show_splash();
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    Ok(())
}

struct AppState {
    hwnd: HWND,
    visible: bool,
    splash_visible: bool,
    mode: OverlayMode,
    all_desktops: bool,
    query: String,
    selected: usize,
    thumbnail_size_index: usize,
    app_icon: HICON,
    owns_app_icon: bool,
    help_hovered: bool,
    close_hovered: bool,
    mouse_tracking: bool,
    help_rect: RECT,
    close_rect: RECT,
    apps: Vec<AppEntry>,
    favorite_folders: Vec<FavoriteFolderEntry>,
    app_scan_completed: bool,
    app_scan_rx: Option<Receiver<Vec<AppEntry>>>,
    scan_apps_after_paint: bool,
    update_check_completed: bool,
    update_check_rx: Option<Receiver<Option<UpdateInfo>>>,
    available_update: Option<UpdateInfo>,
    update_link_hovered: bool,
    update_link_rect: RECT,
    windows: Vec<WindowEntry>,
    accessibility_tabs: Vec<TabEntry>,
    results: Vec<SearchResult>,
    thumbnails: HashMap<isize, isize>,
    original_window_rects: HashMap<isize, RECT>,
    pending_close_windows: HashMap<isize, u8>,
    highlight_windows: [HWND; 4],
    last_peek_target: Option<isize>,
    last_peek_at: Option<Instant>,
    row_layouts: Vec<RowLayout>,
    bridge: ExtensionBridge,
    tray_icon_installed: bool,
}

#[derive(Clone, Copy)]
struct RowLayout {
    index: usize,
    thumbnail: RECT,
}

#[derive(Clone, Copy)]
struct ThumbnailSize {
    width: i32,
    height: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverlayMode {
    WindowsAndTabs,
    Apps,
}

impl AppState {
    fn new(bridge: ExtensionBridge, app_icon: HICON, owns_app_icon: bool) -> Self {
        Self {
            hwnd: HWND(null_mut()),
            visible: false,
            splash_visible: false,
            mode: OverlayMode::WindowsAndTabs,
            all_desktops: false,
            query: String::new(),
            selected: 0,
            thumbnail_size_index: 0,
            app_icon,
            owns_app_icon,
            help_hovered: false,
            close_hovered: false,
            mouse_tracking: false,
            help_rect: RECT::default(),
            close_rect: RECT::default(),
            apps: Vec::new(),
            favorite_folders: load_favorite_folders(),
            app_scan_completed: false,
            app_scan_rx: None,
            scan_apps_after_paint: false,
            update_check_completed: false,
            update_check_rx: None,
            available_update: None,
            update_link_hovered: false,
            update_link_rect: RECT::default(),
            windows: Vec::new(),
            accessibility_tabs: Vec::new(),
            results: Vec::new(),
            thumbnails: HashMap::new(),
            original_window_rects: HashMap::new(),
            pending_close_windows: HashMap::new(),
            highlight_windows: [HWND(null_mut()); 4],
            last_peek_target: None,
            last_peek_at: None,
            row_layouts: Vec::new(),
            bridge,
            tray_icon_installed: false,
        }
    }

    unsafe fn set_hwnd(&mut self, hwnd: HWND) {
        self.hwnd = hwnd;
    }

    unsafe fn install_tray_icon(&mut self) {
        if !self.tray_icon_installed {
            self.tray_icon_installed = install_tray_icon(self.hwnd, self.app_icon);
        }
    }

    unsafe fn show(&mut self) {
        self.visible = true;
        self.splash_visible = false;
        let _ = KillTimer(self.hwnd, SPLASH_TIMER_ID);
        self.mode = OverlayMode::WindowsAndTabs;
        self.all_desktops = false;
        self.query.clear();
        self.selected = 0;
        self.scan_apps_after_paint = !self.app_scan_completed && self.app_scan_rx.is_none();
        self.refresh();

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let width = screen_w.clamp(760, 1120);
        let height = screen_h.clamp(520, 820);
        let left = (screen_w - width) / 2;
        let top = (screen_h - height) / 3;

        SetWindowPos(
            self.hwnd,
            HWND_TOPMOST,
            left,
            top,
            width,
            height,
            SET_WINDOW_POS_FLAGS(0),
        )
        .ok();
        let _ = ShowWindow(self.hwnd, SW_SHOW);
        let _ = SetForegroundWindow(self.hwnd);
        let _ = SetFocus(self.hwnd);
        let _ = InvalidateRect(self.hwnd, None, BOOL(1));
    }

    unsafe fn show_splash(&mut self) {
        self.visible = true;
        self.splash_visible = true;
        self.help_hovered = false;
        self.close_hovered = false;

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let width = 560;
        let height = 240;
        let left = (screen_w - width) / 2;
        let top = (screen_h - height) / 3;

        SetWindowPos(
            self.hwnd,
            HWND_TOPMOST,
            left,
            top,
            width,
            height,
            SET_WINDOW_POS_FLAGS(0),
        )
        .ok();
        let _ = ShowWindow(self.hwnd, SW_SHOW);
        let _ = SetForegroundWindow(self.hwnd);
        let _ = SetFocus(self.hwnd);
        let _ = SetTimer(self.hwnd, SPLASH_TIMER_ID, SPLASH_DURATION_MS, None);
        let _ = InvalidateRect(self.hwnd, None, BOOL(1));
    }

    unsafe fn hide(&mut self) {
        self.visible = false;
        self.splash_visible = false;
        let _ = KillTimer(self.hwnd, SPLASH_TIMER_ID);
        self.mode = OverlayMode::WindowsAndTabs;
        self.all_desktops = false;
        self.query.clear();
        self.selected = 0;
        self.help_hovered = false;
        self.close_hovered = false;
        self.mouse_tracking = false;
        self.results.clear();
        self.row_layouts.clear();
        self.original_window_rects.clear();
        self.pending_close_windows.clear();
        self.scan_apps_after_paint = false;
        self.last_peek_target = None;
        self.last_peek_at = None;
        let _ = KillTimer(self.hwnd, CLOSE_REFRESH_TIMER_ID);
        self.hide_attention_border();
        self.unregister_thumbnails();
        let _ = ShowWindow(self.hwnd, SW_HIDE);
    }

    unsafe fn toggle(&mut self) {
        if self.visible && !self.splash_visible {
            self.hide();
        } else {
            self.show();
        }
    }

    unsafe fn refresh(&mut self) {
        self.unregister_thumbnails();
        self.windows = enumerate_windows(self.hwnd, self.all_desktops);
        self.accessibility_tabs = scan_chrome_tabs(&self.windows);
        self.rebuild_results();
    }

    unsafe fn start_app_scan_if_needed(&mut self) {
        if self.app_scan_completed || self.app_scan_rx.is_some() {
            return;
        }

        let (sender, receiver) = mpsc::channel();
        self.app_scan_rx = Some(receiver);
        let _ = SetTimer(self.hwnd, APP_SCAN_TIMER_ID, APP_SCAN_POLL_MS, None);
        let _ = thread::spawn(move || {
            let _ = sender.send(enumerate_apps());
        });
    }

    unsafe fn poll_app_scan(&mut self) {
        let Some(scan_result) = self
            .app_scan_rx
            .as_ref()
            .map(|receiver| receiver.try_recv())
        else {
            return;
        };

        match scan_result {
            Ok(apps) => {
                self.apps = apps;
                self.app_scan_completed = true;
                self.app_scan_rx = None;
                let _ = KillTimer(self.hwnd, APP_SCAN_TIMER_ID);
                if self.mode == OverlayMode::Apps {
                    self.rebuild_results();
                    let _ = InvalidateRect(self.hwnd, None, BOOL(1));
                }
            }
            Err(TryRecvError::Disconnected) => {
                self.app_scan_completed = true;
                self.app_scan_rx = None;
                let _ = KillTimer(self.hwnd, APP_SCAN_TIMER_ID);
                if self.mode == OverlayMode::Apps {
                    self.rebuild_results();
                    let _ = InvalidateRect(self.hwnd, None, BOOL(1));
                }
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    unsafe fn start_update_check_if_needed(&mut self) {
        if self.update_check_completed || self.update_check_rx.is_some() {
            return;
        }

        let (sender, receiver) = mpsc::channel();
        self.update_check_rx = Some(receiver);
        let _ = SetTimer(self.hwnd, UPDATE_CHECK_TIMER_ID, UPDATE_CHECK_POLL_MS, None);
        let _ = thread::spawn(move || {
            let _ = sender.send(check_for_update(APP_VERSION));
        });
    }

    unsafe fn poll_update_check(&mut self) {
        let Some(update_result) = self
            .update_check_rx
            .as_ref()
            .map(|receiver| receiver.try_recv())
        else {
            return;
        };

        match update_result {
            Ok(update) => {
                self.available_update = update;
                self.update_check_completed = true;
                self.update_check_rx = None;
                let _ = KillTimer(self.hwnd, UPDATE_CHECK_TIMER_ID);
                if self.available_update.is_some() && self.visible && !self.splash_visible {
                    if self.tray_icon_installed {
                        if let Some(update) = &self.available_update {
                            show_update_notification(self.hwnd, &update.latest_version);
                        }
                    }
                    let _ = InvalidateRect(self.hwnd, None, BOOL(1));
                }
            }
            Err(TryRecvError::Disconnected) => {
                self.update_check_completed = true;
                self.update_check_rx = None;
                let _ = KillTimer(self.hwnd, UPDATE_CHECK_TIMER_ID);
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    fn rebuild_results(&mut self) {
        self.results = match self.mode {
            OverlayMode::WindowsAndTabs => build_results_with_options(
                &self.query,
                &self.windows,
                &self.accessibility_tabs,
                &self.bridge.tabs(),
                BuildResultOptions {
                    show_desktop_labels: self.all_desktops,
                },
            ),
            OverlayMode::Apps => {
                build_launcher_results(&self.query, &self.apps, &self.favorite_folders)
            }
        };
        if self.results.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.results.len() {
            self.selected = self.results.len() - 1;
        }
    }

    unsafe fn on_char(&mut self, ch: char) {
        if !self.visible || self.splash_visible {
            return;
        }
        if is_control_down() {
            return;
        }
        if ch >= ' ' && ch != '\u{7f}' {
            self.query.push(ch);
            self.selected = 0;
            self.rebuild_results();
            let _ = InvalidateRect(self.hwnd, None, BOOL(1));
        }
    }

    unsafe fn on_key_down(&mut self, key: u32) -> DeferredAction {
        if !self.visible {
            return DeferredAction::None;
        }
        let ctrl_down = is_control_down();
        if self.splash_visible {
            match key {
                key if key == VK_ESCAPE.0 as u32 => self.hide(),
                key if key == VK_RETURN.0 as u32 || key == VK_SPACE.0 as u32 => self.show(),
                _ => {}
            }
            return DeferredAction::None;
        }

        match key {
            key if key == VK_ESCAPE.0 as u32 => self.hide(),
            key if key == VK_BACK.0 as u32 => {
                self.query.pop();
                self.selected = 0;
                self.rebuild_results();
                let _ = InvalidateRect(self.hwnd, None, BOOL(1));
            }
            key if ctrl_down && key == KEY_F => return self.selected_toggle_maximize(),
            key if ctrl_down && key == KEY_S => return self.selected_minimize_window(),
            key if ctrl_down && key == KEY_W => return self.selected_close_window(),
            key if ctrl_down && is_app_mode_toggle_key(key) => self.toggle_app_mode(),
            key if ctrl_down
                && self.mode == OverlayMode::WindowsAndTabs
                && key == VK_D.0 as u32 =>
            {
                self.toggle_all_desktops()
            }
            key if ctrl_down && key == VK_RIGHT.0 as u32 => {
                return self.selected_move_to_monitor(MonitorMoveDirection::Next);
            }
            key if ctrl_down && key == VK_LEFT.0 as u32 => {
                return self.selected_move_to_monitor(MonitorMoveDirection::Previous);
            }
            key if key == VK_UP.0 as u32 => {
                if !self.results.is_empty() {
                    self.selected = self.selected.saturating_sub(1);
                    let _ = InvalidateRect(self.hwnd, None, BOOL(1));
                }
            }
            key if key == VK_DOWN.0 as u32 || key == VK_TAB.0 as u32 => {
                if !self.results.is_empty() {
                    self.selected = (self.selected + 1).min(self.results.len() - 1);
                    let _ = InvalidateRect(self.hwnd, None, BOOL(1));
                }
            }
            key if self.mode == OverlayMode::WindowsAndTabs
                && (key == VK_LEFT.0 as u32 || key == VK_RIGHT.0 as u32) =>
            {
                return self.selected_peek_window();
            }
            key if key == VK_RETURN.0 as u32 => {
                let action = self.selected_activation(false);
                self.hide();
                return action;
            }
            _ => {}
        }

        DeferredAction::None
    }

    unsafe fn toggle_app_mode(&mut self) {
        self.mode = match self.mode {
            OverlayMode::WindowsAndTabs => {
                self.start_app_scan_if_needed();
                OverlayMode::Apps
            }
            OverlayMode::Apps => OverlayMode::WindowsAndTabs,
        };
        self.selected = 0;
        self.unregister_thumbnails();
        self.rebuild_results();
        let _ = InvalidateRect(self.hwnd, None, BOOL(1));
    }

    unsafe fn toggle_all_desktops(&mut self) {
        self.all_desktops = !self.all_desktops;
        self.selected = 0;
        self.unregister_thumbnails();
        self.refresh();
        let _ = InvalidateRect(self.hwnd, None, BOOL(1));
    }

    fn thumbnail_size(&self) -> ThumbnailSize {
        THUMBNAIL_SIZES[self.thumbnail_size_index]
    }

    fn empty_message(&self) -> &'static str {
        match (self.mode, self.all_desktops) {
            (OverlayMode::WindowsAndTabs, false) => "No matching windows or Chrome tabs",
            (OverlayMode::WindowsAndTabs, true) => {
                "No matching windows or Chrome tabs on any desktop"
            }
            (OverlayMode::Apps, _) if self.app_scan_rx.is_some() => {
                "Scanning installed apps and favorite folders..."
            }
            (OverlayMode::Apps, _) => "No matching installed apps or favorite folders",
        }
    }

    fn selected_activation(&self, restore_overlay_focus: bool) -> DeferredAction {
        let Some(result) = self.results.get(self.selected).cloned() else {
            return DeferredAction::None;
        };

        DeferredAction::Activate(Box::new(ActivationRequest {
            result,
            windows: self.windows.clone(),
            bridge: self.bridge.clone(),
            overlay_hwnd: self.hwnd,
            restore_overlay_focus,
            highlight_target: None,
        }))
    }

    fn selected_peek_window(&mut self) -> DeferredAction {
        let Some(result) = self.results.get(self.selected).cloned() else {
            return DeferredAction::None;
        };
        let Some(peek_target) = selected_move_target_hwnd(&result, &self.windows) else {
            return DeferredAction::None;
        };
        let now = Instant::now();
        let highlight_target = should_highlight_repeated_peek(
            self.last_peek_target,
            self.last_peek_at,
            Some(peek_target),
            now,
        )
        .then_some(peek_target);
        self.last_peek_target = Some(peek_target);
        self.last_peek_at = Some(now);

        DeferredAction::PeekWindow(Box::new(PeekWindowRequest {
            hwnd: peek_target,
            overlay_hwnd: self.hwnd,
            highlight_target,
        }))
    }

    fn selected_toggle_maximize(&self) -> DeferredAction {
        let Some(result) = self.results.get(self.selected).cloned() else {
            return DeferredAction::None;
        };
        let Some(hwnd) = selected_move_target_hwnd(&result, &self.windows) else {
            return DeferredAction::None;
        };

        DeferredAction::ToggleMaximize(Box::new(ToggleMaximizeRequest {
            hwnd,
            overlay_hwnd: self.hwnd,
        }))
    }

    fn selected_minimize_window(&self) -> DeferredAction {
        let Some(result) = self.results.get(self.selected).cloned() else {
            return DeferredAction::None;
        };
        let Some(hwnd) = selected_move_target_hwnd(&result, &self.windows) else {
            return DeferredAction::None;
        };

        DeferredAction::MinimizeWindow(Box::new(MinimizeWindowRequest {
            hwnd,
            overlay_hwnd: self.hwnd,
        }))
    }

    unsafe fn selected_close_window(&mut self) -> DeferredAction {
        if let Some(path) =
            selected_folder_removal_target(self.mode, self.results.get(self.selected))
        {
            if remove_favorite_folder_path(&mut self.favorite_folders, &path) {
                let _ = save_favorite_folders(&self.favorite_folders);
                self.rebuild_results();
                let _ = InvalidateRect(self.hwnd, None, BOOL(1));
            }
            return DeferredAction::None;
        }

        let Some(result) = self.results.get(self.selected).cloned() else {
            return DeferredAction::None;
        };
        let Some(hwnd) = selected_move_target_hwnd(&result, &self.windows) else {
            return DeferredAction::None;
        };

        self.track_pending_close(hwnd);

        DeferredAction::CloseWindow(Box::new(CloseWindowRequest {
            hwnd,
            overlay_hwnd: self.hwnd,
        }))
    }

    unsafe fn track_pending_close(&mut self, hwnd: isize) {
        self.pending_close_windows
            .insert(hwnd, CLOSE_REFRESH_ATTEMPTS);
        let _ = SetTimer(
            self.hwnd,
            CLOSE_REFRESH_TIMER_ID,
            CLOSE_REFRESH_POLL_MS,
            None,
        );
    }

    unsafe fn poll_pending_close_windows(&mut self) {
        let mut closed_window_removed = false;
        let self_hwnd = self.hwnd;
        let include_all_desktops = self.all_desktops;
        let virtual_desktop_manager = virtual_desktop_manager();
        self.pending_close_windows.retain(|hwnd, attempts| {
            let still_listable = pending_close_window_still_listable(
                hwnd_from_isize(*hwnd),
                self_hwnd,
                include_all_desktops,
                virtual_desktop_manager.as_ref(),
            );
            match pending_close_poll_decision(still_listable, *attempts) {
                PendingClosePollDecision::Keep(next_attempts) => {
                    *attempts = next_attempts;
                    true
                }
                PendingClosePollDecision::RemoveAndRefresh => {
                    closed_window_removed = true;
                    false
                }
                PendingClosePollDecision::RemoveQuietly => false,
            }
        });

        if self.pending_close_windows.is_empty() {
            let _ = KillTimer(self.hwnd, CLOSE_REFRESH_TIMER_ID);
        }

        if closed_window_removed && self.visible && !self.splash_visible {
            self.refresh();
            let _ = InvalidateRect(self.hwnd, None, BOOL(1));
        }
    }

    unsafe fn selected_move_to_monitor(
        &mut self,
        direction: MonitorMoveDirection,
    ) -> DeferredAction {
        let Some(result) = self.results.get(self.selected).cloned() else {
            return DeferredAction::None;
        };
        let Some(hwnd) = selected_move_target_hwnd(&result, &self.windows) else {
            return DeferredAction::None;
        };
        let original_rect = match self.original_window_rects.get(&hwnd).copied() {
            Some(rect) => rect,
            None => {
                let Some(rect) = current_window_rect(hwnd_from_isize(hwnd)) else {
                    return DeferredAction::None;
                };
                self.original_window_rects.insert(hwnd, rect);
                rect
            }
        };

        DeferredAction::MoveToMonitor(Box::new(MoveWindowRequest {
            hwnd,
            overlay_hwnd: self.hwnd,
            original_rect,
            direction,
        }))
    }

    unsafe fn show_attention_border(&mut self, target: isize) {
        let target_hwnd = hwnd_from_isize(target);
        if !IsWindow(target_hwnd).as_bool() {
            return;
        }
        let Some(rect) = highlight_rect_for_window(target_hwnd) else {
            return;
        };
        let bounds = monitor_rect_for_window(target_hwnd).unwrap_or(rect);
        if !self.ensure_highlight_windows() {
            return;
        }

        let positions = attention_border_positions(rect, bounds, HIGHLIGHT_BORDER_PX);

        for (window, rect) in self.highlight_windows.iter().copied().zip(positions) {
            let _ = SetWindowPos(
                window,
                HWND_TOPMOST,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOACTIVATE,
            );
            let _ = ShowWindow(window, SW_SHOW);
            let _ = InvalidateRect(window, None, BOOL(1));
        }
        let _ = SetTimer(self.hwnd, HIGHLIGHT_TIMER_ID, HIGHLIGHT_DURATION_MS, None);
        restore_overlay_focus(self.hwnd);
    }

    unsafe fn hide_attention_border(&mut self) {
        let _ = KillTimer(self.hwnd, HIGHLIGHT_TIMER_ID);
        for window in self.highlight_windows.iter().copied() {
            if !window.0.is_null() {
                let _ = ShowWindow(window, SW_HIDE);
            }
        }
    }

    unsafe fn destroy_highlight_windows(&mut self) {
        let _ = KillTimer(self.hwnd, HIGHLIGHT_TIMER_ID);
        for window in self.highlight_windows.iter_mut() {
            if !window.0.is_null() {
                let _ = DestroyWindow(*window);
                *window = HWND(null_mut());
            }
        }
    }

    unsafe fn ensure_highlight_windows(&mut self) -> bool {
        let Ok(instance) = windows::Win32::System::LibraryLoader::GetModuleHandleW(None) else {
            return false;
        };

        for window in self.highlight_windows.iter_mut() {
            if !window.0.is_null() && IsWindow(*window).as_bool() {
                continue;
            }

            let Ok(created) = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                HIGHLIGHT_CLASS_NAME,
                w!("Mega Win Alt Tab Highlight"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                instance,
                None,
            ) else {
                return false;
            };
            *window = created;
        }

        true
    }

    unsafe fn remove_tray_icon(&mut self) {
        if self.tray_icon_installed {
            remove_tray_icon(self.hwnd);
            self.tray_icon_installed = false;
        }
    }

    unsafe fn paint(&mut self) {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(self.hwnd, &mut ps);
        let mut rect = RECT::default();
        let _ = GetClientRect(self.hwnd, &mut rect);

        fill_rect(hdc, rect, rgb(24, 26, 27));
        if self.splash_visible {
            self.draw_splash(hdc, rect);
        } else {
            self.draw_search(hdc, rect);
            self.draw_results(hdc, rect);
            if self.help_hovered {
                self.draw_help_tooltip(hdc, rect);
            }
            self.update_thumbnails();
            if self.scan_apps_after_paint {
                self.scan_apps_after_paint = false;
                self.start_app_scan_if_needed();
            }
            self.start_update_check_if_needed();
        }

        let _ = EndPaint(self.hwnd, &ps);
    }

    unsafe fn draw_splash(&self, hdc: HDC, rect: RECT) {
        let icon_size = 48;
        let icon_left = rect.left + 34;
        let icon_top = rect.top + 34;
        let _ = DrawIconEx(
            hdc,
            icon_left,
            icon_top,
            self.app_icon,
            icon_size,
            icon_size,
            0,
            HBRUSH(null_mut()),
            DI_NORMAL,
        );

        let title_font = make_font(28, FW_BOLD.0 as i32);
        let old_title = SelectObject(hdc, title_font);
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, rgb(247, 250, 252));
        draw_text(
            hdc,
            "Mega Win Alt Tab",
            RECT {
                left: icon_left + icon_size + 18,
                top: rect.top + 30,
                right: rect.right - 28,
                bottom: rect.top + 66,
            },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(hdc, old_title);
        let _ = DeleteObject(title_font);

        let subtitle_font = make_font(17, FW_NORMAL.0 as i32);
        let old_subtitle = SelectObject(hdc, subtitle_font);
        SetTextColor(hdc, rgb(190, 198, 206));
        draw_text(
            hdc,
            "Search windows, Chrome tabs, and installed apps.",
            RECT {
                left: icon_left + icon_size + 18,
                top: rect.top + 70,
                right: rect.right - 28,
                bottom: rect.top + 98,
            },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(hdc, old_subtitle);
        let _ = DeleteObject(subtitle_font);

        let hint_font = make_font(16, FW_NORMAL.0 as i32);
        let old_hint = SelectObject(hdc, hint_font);
        SetTextColor(hdc, rgb(232, 236, 241));
        draw_text(
            hdc,
            "Ctrl+Alt+Space opens the switcher",
            RECT {
                left: rect.left + 34,
                top: rect.top + 126,
                right: rect.right - 34,
                bottom: rect.top + 154,
            },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SetTextColor(hdc, rgb(160, 170, 181));
        draw_text(
            hdc,
            "The tray icon stays running in the notification area.",
            RECT {
                left: rect.left + 34,
                top: rect.top + 158,
                right: rect.right - 34,
                bottom: rect.top + 186,
            },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(hdc, old_hint);
        let _ = DeleteObject(hint_font);
    }

    unsafe fn draw_search(&mut self, hdc: HDC, rect: RECT) {
        let search_rect = RECT {
            left: rect.left + 28,
            top: rect.top + 22,
            right: rect.right - 28,
            bottom: rect.top + 78,
        };
        fill_rect(hdc, search_rect, rgb(42, 45, 48));

        let icon_size = 36;
        let icon_rect = RECT {
            left: search_rect.left + 14,
            top: search_rect.top + 10,
            right: search_rect.left + 14 + icon_size,
            bottom: search_rect.top + 10 + icon_size,
        };
        let _ = DrawIconEx(
            hdc,
            icon_rect.left,
            icon_rect.top,
            self.app_icon,
            icon_size,
            icon_size,
            0,
            HBRUSH(null_mut()),
            DI_NORMAL,
        );

        let buttons = search_button_rects(search_rect);
        self.help_rect = buttons.help;
        self.close_rect = buttons.close;
        draw_help_button(hdc, self.help_rect, self.help_hovered);
        draw_close_button(hdc, self.close_rect, self.close_hovered);

        let label_font = make_font(24, FW_NORMAL.0 as i32);
        let old = SelectObject(hdc, label_font);
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, rgb(236, 238, 240));

        let text = match (self.mode, self.all_desktops, self.query.is_empty()) {
            (OverlayMode::WindowsAndTabs, false, true) => {
                "Type to search windows and Chrome tabs".to_string()
            }
            (OverlayMode::WindowsAndTabs, false, false) => format!("Search: {}", self.query),
            (OverlayMode::WindowsAndTabs, true, true) => "Type to search all desktops".to_string(),
            (OverlayMode::WindowsAndTabs, true, false) => {
                format!("All desktops: {}", self.query)
            }
            (OverlayMode::Apps, _, true) => {
                "Type to search installed apps and favorite folders".to_string()
            }
            (OverlayMode::Apps, _, false) => format!("Apps: {}", self.query),
        };
        draw_text(
            hdc,
            &text,
            RECT {
                left: icon_rect.right + 14,
                top: search_rect.top,
                right: self.help_rect.left - 14,
                bottom: search_rect.bottom,
            },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );

        let counter_font = make_font(14, FW_NORMAL.0 as i32);
        let old_counter = SelectObject(hdc, counter_font);
        if let Some(update) = &self.available_update {
            self.update_link_rect = RECT {
                left: search_rect.left + 12,
                top: search_rect.bottom + 5,
                right: search_rect.right - 120,
                bottom: search_rect.bottom + 23,
            };
            SetTextColor(
                hdc,
                if self.update_link_hovered {
                    rgb(255, 232, 126)
                } else {
                    rgb(255, 208, 64)
                },
            );
            draw_text(
                hdc,
                &update_link_text(&update.latest_version),
                self.update_link_rect,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
            if self.update_link_hovered {
                fill_rect(
                    hdc,
                    RECT {
                        left: self.update_link_rect.left,
                        top: self.update_link_rect.bottom - 2,
                        right: self.update_link_rect.right,
                        bottom: self.update_link_rect.bottom - 1,
                    },
                    rgb(255, 232, 126),
                );
            }
        } else {
            self.update_link_rect = RECT::default();
        }
        SetTextColor(hdc, rgb(168, 176, 184));
        draw_text(
            hdc,
            &selection_status_text(self.selected, self.results.len()),
            RECT {
                left: search_rect.left + 12,
                top: search_rect.bottom + 5,
                right: search_rect.right - 8,
                bottom: search_rect.bottom + 23,
            },
            DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        SelectObject(hdc, old_counter);
        let _ = DeleteObject(counter_font);

        SelectObject(hdc, old);
        let _ = DeleteObject(label_font);
    }

    unsafe fn draw_results(&mut self, hdc: HDC, rect: RECT) {
        self.row_layouts.clear();
        let mut top = rect.top + 112;
        let gap = 6;
        let row_left = rect.left + 28;
        let row_right = rect.right - 28;
        let number_col_width = result_number_column_width(self.results.len());
        let thumbnail_size = match self.mode {
            OverlayMode::WindowsAndTabs => self.thumbnail_size(),
            OverlayMode::Apps => THUMBNAIL_SIZES[0],
        };
        let thumb_w = thumbnail_size
            .width
            .min((row_right - row_left - number_col_width - 120).max(112));
        let thumb_h = thumbnail_size.height;
        let row_height = thumb_h + 32;
        let visible_rows = visible_result_count(rect, top, row_height, gap, self.results.len());
        let start_index = visible_result_start(self.selected, visible_rows);

        let title_font = make_font(19, FW_BOLD.0 as i32);
        let subtitle_font = make_font(15, FW_NORMAL.0 as i32);
        let number_font = make_font(17, FW_BOLD.0 as i32);
        let screen_font = make_font(12, FW_BOLD.0 as i32);

        for (index, result) in self
            .results
            .iter()
            .enumerate()
            .skip(start_index)
            .take(visible_rows)
        {
            let row = RECT {
                left: row_left,
                top,
                right: row_right,
                bottom: top + row_height,
            };
            let number_rect = RECT {
                left: row.left + 8,
                top: row.top,
                right: row.left + number_col_width,
                bottom: row.bottom,
            };
            let thumb = RECT {
                left: row.left + number_col_width + 8,
                top: row.top + 7,
                right: row.left + number_col_width + 8 + thumb_w,
                bottom: row.top + 7 + thumb_h,
            };
            let selected = index == self.selected;

            fill_rect(
                hdc,
                row,
                if selected {
                    rgb(62, 89, 122)
                } else {
                    rgb(35, 38, 41)
                },
            );
            fill_rect(hdc, thumb, rgb(18, 20, 22));

            let old_number = SelectObject(hdc, number_font);
            SetTextColor(
                hdc,
                if selected {
                    rgb(250, 252, 255)
                } else {
                    rgb(154, 164, 174)
                },
            );
            draw_text(
                hdc,
                &result_number_label(index),
                number_rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
            SelectObject(hdc, old_number);

            self.row_layouts.push(RowLayout {
                index,
                thumbnail: thumb,
            });

            let text_left = thumb.right + 16;
            let kind_label = match result.kind {
                SearchResultKind::Window => "Window",
                SearchResultKind::Tab => "Chrome tab",
                SearchResultKind::App => "Application",
                SearchResultKind::Folder => "Favorite folder",
            };

            if matches!(
                result.kind,
                SearchResultKind::App | SearchResultKind::Folder
            ) {
                let old_app = SelectObject(hdc, screen_font);
                SetTextColor(hdc, rgb(188, 194, 200));
                let placeholder = if result.kind == SearchResultKind::Folder {
                    "FOLDER"
                } else {
                    "APP"
                };
                draw_text(
                    hdc,
                    placeholder,
                    thumb,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
                SelectObject(hdc, old_app);
            }

            let old_title = SelectObject(hdc, title_font);
            SetTextColor(hdc, rgb(247, 248, 249));
            draw_text(
                hdc,
                &result.title,
                RECT {
                    left: text_left,
                    top: row.top + 12,
                    right: row.right - 14,
                    bottom: row.top + 38,
                },
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
            SelectObject(hdc, old_title);

            let old_subtitle = SelectObject(hdc, subtitle_font);
            SetTextColor(hdc, rgb(188, 194, 200));
            draw_text(
                hdc,
                &format!("{kind_label} - {}", result.subtitle),
                RECT {
                    left: text_left,
                    top: row.top + 40,
                    right: row.right - 14,
                    bottom: row.top + 64,
                },
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
            SelectObject(hdc, old_subtitle);

            if let Some(screen_number) = result.screen_number {
                let badge = RECT {
                    left: thumb.left + 44,
                    top: thumb.bottom + 4,
                    right: thumb.left + 68,
                    bottom: thumb.bottom + 22,
                };
                fill_rect(
                    hdc,
                    badge,
                    if selected {
                        rgb(82, 112, 150)
                    } else {
                        rgb(52, 57, 62)
                    },
                );

                let old_screen = SelectObject(hdc, screen_font);
                SetTextColor(hdc, rgb(247, 248, 249));
                draw_text(
                    hdc,
                    &screen_number.to_string(),
                    badge,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
                SelectObject(hdc, old_screen);
            }

            top += row_height + gap;
        }

        if self.results.is_empty() {
            let empty_font = make_font(20, FW_NORMAL.0 as i32);
            let old = SelectObject(hdc, empty_font);
            SetTextColor(hdc, rgb(188, 194, 200));
            draw_text(
                hdc,
                self.empty_message(),
                RECT {
                    left: row_left,
                    top,
                    right: row_right,
                    bottom: top + 50,
                },
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
            SelectObject(hdc, old);
            let _ = DeleteObject(empty_font);
        }

        let _ = DeleteObject(title_font);
        let _ = DeleteObject(subtitle_font);
        let _ = DeleteObject(number_font);
        let _ = DeleteObject(screen_font);
    }

    unsafe fn draw_help_tooltip(&self, hdc: HDC, rect: RECT) {
        let line_height = 22;
        let width = 382;
        let height = 18 + (HELP_LINES.len() as i32 * line_height);
        let right = (self.help_rect.right + 6).min(rect.right - 28);
        let left = (right - width).max(rect.left + 28);
        let top = self.help_rect.bottom + 8;
        let tooltip = RECT {
            left,
            top,
            right,
            bottom: top + height,
        };

        fill_rect(hdc, tooltip, rgb(36, 39, 42));
        draw_rect_outline(hdc, tooltip, rgb(86, 96, 108));

        let font = make_font(15, FW_NORMAL.0 as i32);
        let old = SelectObject(hdc, font);
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, rgb(238, 241, 243));
        for (index, line) in HELP_LINES.iter().enumerate() {
            let line_top = tooltip.top + 9 + index as i32 * line_height;
            draw_text(
                hdc,
                line,
                RECT {
                    left: tooltip.left + 14,
                    top: line_top,
                    right: tooltip.right - 14,
                    bottom: line_top + line_height,
                },
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
        }
        SelectObject(hdc, old);
        let _ = DeleteObject(font);
    }

    unsafe fn update_thumbnails(&mut self) {
        let mut visible_hwnds = HashSet::new();

        for layout in &self.row_layouts {
            let Some(result) = self.results.get(layout.index) else {
                continue;
            };
            let hwnd = result_thumbnail_hwnd(result);
            let Some(hwnd) = hwnd else {
                continue;
            };
            if hwnd == hwnd_to_isize(self.hwnd) || !IsWindow(hwnd_from_isize(hwnd)).as_bool() {
                continue;
            }

            visible_hwnds.insert(hwnd);
            let thumbnail = match self.thumbnails.get(&hwnd).copied() {
                Some(thumbnail) => thumbnail,
                None => {
                    let Ok(thumbnail) = DwmRegisterThumbnail(self.hwnd, hwnd_from_isize(hwnd))
                    else {
                        continue;
                    };
                    self.thumbnails.insert(hwnd, thumbnail);
                    thumbnail
                }
            };

            let props = DWM_THUMBNAIL_PROPERTIES {
                dwFlags: DWM_TNP_VISIBLE | DWM_TNP_RECTDESTINATION | DWM_TNP_OPACITY,
                rcDestination: layout.thumbnail,
                opacity: 220,
                fVisible: BOOL(1),
                fSourceClientAreaOnly: BOOL(0),
                ..Default::default()
            };
            let _ = DwmUpdateThumbnailProperties(thumbnail, &props);
        }

        let stale = self
            .thumbnails
            .keys()
            .copied()
            .filter(|hwnd| !visible_hwnds.contains(hwnd))
            .collect::<Vec<_>>();
        for hwnd in stale {
            if let Some(thumbnail) = self.thumbnails.remove(&hwnd) {
                let _ = DwmUnregisterThumbnail(thumbnail);
            }
        }
    }

    unsafe fn unregister_thumbnails(&mut self) {
        for (_, thumbnail) in self.thumbnails.drain() {
            let _ = DwmUnregisterThumbnail(thumbnail);
        }
    }

    unsafe fn on_mouse_move(&mut self, x: i32, y: i32) {
        if !self.visible {
            return;
        }

        if !self.mouse_tracking {
            let mut event = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.hwnd,
                dwHoverTime: 0,
            };
            self.mouse_tracking = TrackMouseEvent(&mut event).is_ok();
        }

        let help_hovered = point_in_rect(self.help_rect, x, y);
        let close_hovered = point_in_rect(self.close_rect, x, y);
        let update_link_hovered =
            self.available_update.is_some() && point_in_rect(self.update_link_rect, x, y);
        if help_hovered != self.help_hovered
            || close_hovered != self.close_hovered
            || update_link_hovered != self.update_link_hovered
        {
            self.help_hovered = help_hovered;
            self.close_hovered = close_hovered;
            self.update_link_hovered = update_link_hovered;
            let _ = InvalidateRect(self.hwnd, None, BOOL(1));
        }
    }

    unsafe fn on_left_button_up(&mut self, x: i32, y: i32) {
        if self.visible
            && self.available_update.is_some()
            && point_in_rect(self.update_link_rect, x, y)
        {
            open_github_releases(self.hwnd);
            return;
        }
        if self.visible && point_in_rect(self.close_rect, x, y) {
            self.hide();
        }
    }

    unsafe fn on_drop_files(&mut self, hdrop: HDROP) {
        let mut first_valid_folder = None;
        let mut changed = false;
        for path in dropped_file_paths(hdrop) {
            if let Some(result) = add_favorite_folder_path(&mut self.favorite_folders, &path) {
                changed |= result.added;
                if first_valid_folder.is_none() {
                    first_valid_folder = Some(result.entry);
                }
            }
        }

        let Some(folder) = first_valid_folder else {
            return;
        };

        if changed {
            let _ = save_favorite_folders(&self.favorite_folders);
        }
        self.mode = OverlayMode::Apps;
        self.query = folder.name.clone();
        self.selected = 0;
        self.start_app_scan_if_needed();
        self.unregister_thumbnails();
        self.rebuild_results();
        self.select_folder_result(&folder.path);
        let _ = InvalidateRect(self.hwnd, None, BOOL(1));
    }

    fn select_folder_result(&mut self, path: &str) {
        let normalized = normalize_folder_path(path);
        self.selected = self
            .results
            .iter()
            .position(|result| match &result.target {
                ActivationTarget::Folder { path } => normalize_folder_path(path) == normalized,
                _ => false,
            })
            .unwrap_or(0);
    }

    unsafe fn on_mouse_leave(&mut self) {
        self.mouse_tracking = false;
        if self.help_hovered || self.close_hovered || self.update_link_hovered {
            self.help_hovered = false;
            self.close_hovered = false;
            self.update_link_hovered = false;
            let _ = InvalidateRect(self.hwnd, None, BOOL(1));
        }
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        unsafe {
            self.remove_tray_icon();
            self.unregister_thumbnails();
            self.destroy_highlight_windows();
            if !self.hwnd.0.is_null() {
                DragAcceptFiles(self.hwnd, false);
                let _ = UnregisterHotKey(self.hwnd, HOTKEY_ID);
            }
            if self.owns_app_icon {
                let _ = DestroyIcon(self.app_icon);
            }
        }
    }
}

enum DeferredAction {
    None,
    Activate(Box<ActivationRequest>),
    CloseWindow(Box<CloseWindowRequest>),
    MinimizeWindow(Box<MinimizeWindowRequest>),
    MoveToMonitor(Box<MoveWindowRequest>),
    PeekWindow(Box<PeekWindowRequest>),
    ToggleMaximize(Box<ToggleMaximizeRequest>),
}

struct ActivationRequest {
    result: SearchResult,
    windows: Vec<WindowEntry>,
    bridge: ExtensionBridge,
    overlay_hwnd: HWND,
    restore_overlay_focus: bool,
    highlight_target: Option<isize>,
}

struct MoveWindowRequest {
    hwnd: isize,
    overlay_hwnd: HWND,
    original_rect: RECT,
    direction: MonitorMoveDirection,
}

struct PeekWindowRequest {
    hwnd: isize,
    overlay_hwnd: HWND,
    highlight_target: Option<isize>,
}

struct CloseWindowRequest {
    hwnd: isize,
    overlay_hwnd: HWND,
}

struct MinimizeWindowRequest {
    hwnd: isize,
    overlay_hwnd: HWND,
}

struct ToggleMaximizeRequest {
    hwnd: isize,
    overlay_hwnd: HWND,
}

impl DeferredAction {
    unsafe fn run(self) -> DeferredOutcome {
        match self {
            Self::None => DeferredOutcome::default(),
            Self::Activate(request) => DeferredOutcome {
                highlight_target: run_activation(*request),
                ..Default::default()
            },
            Self::CloseWindow(request) => {
                let _ = run_close_window(*request);
                DeferredOutcome::default()
            }
            Self::MinimizeWindow(request) => {
                run_minimize_window(*request);
                DeferredOutcome::default()
            }
            Self::MoveToMonitor(request) => {
                let _ = run_move_to_monitor(*request);
                DeferredOutcome::default()
            }
            Self::PeekWindow(request) => DeferredOutcome {
                highlight_target: run_peek_window(*request),
                ..Default::default()
            },
            Self::ToggleMaximize(request) => {
                run_toggle_maximize(*request);
                DeferredOutcome::default()
            }
        }
    }
}

#[derive(Default)]
struct DeferredOutcome {
    refresh: bool,
    highlight_target: Option<isize>,
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match catch_unwind(AssertUnwindSafe(|| unsafe {
        wnd_proc_inner(hwnd, message, wparam, lparam)
    })) {
        Ok(result) => result,
        Err(_) => {
            log_runtime_issue("Recovered from a panic while processing a Windows message.");
            LRESULT(0)
        }
    }
}

unsafe extern "system" fn highlight_wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            fill_rect(hdc, rect, rgb(255, 208, 64));
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn wnd_proc_inner(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        let create = lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW;
        let state = (*create).lpCreateParams as *const RefCell<AppState>;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
        (*state).borrow_mut().set_hwnd(hwnd);
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }

    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<AppState>;
    if state_ptr.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }

    match message {
        WM_HOTKEY => {
            if wparam.0 as i32 == HOTKEY_ID {
                with_state_mut(state_ptr, "toggle overlay from hotkey", (), |state| {
                    state.toggle()
                });
                return LRESULT(0);
            }
        }
        WM_TRAYICON => {
            if is_tray_icon_message(wparam, lparam) {
                if is_tray_select_event(lparam) {
                    with_state_mut(state_ptr, "show overlay from tray icon", (), |state| {
                        state.show()
                    });
                } else if is_tray_context_event(lparam) {
                    handle_tray_menu_command(hwnd);
                }
            }
            return LRESULT(0);
        }
        WM_KEYDOWN => {
            let action = with_state_mut(
                state_ptr,
                "handle key press",
                DeferredAction::None,
                |state| state.on_key_down(wparam.0 as u32),
            );
            let outcome = action.run();
            if outcome.refresh {
                with_state_mut(state_ptr, "refresh after deferred action", (), |state| {
                    state.refresh()
                });
            }
            if let Some(target) = outcome.highlight_target {
                with_state_mut(state_ptr, "show attention border", (), |state| {
                    state.show_attention_border(target)
                });
            }
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            let (x, y) = mouse_point(lparam);
            with_state_mut(state_ptr, "handle mouse move", (), |state| {
                state.on_mouse_move(x, y)
            });
            return LRESULT(0);
        }
        WM_MOUSELEAVE_MESSAGE => {
            with_state_mut(state_ptr, "handle mouse leave", (), |state| {
                state.on_mouse_leave()
            });
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            let (x, y) = mouse_point(lparam);
            with_state_mut(state_ptr, "handle left click", (), |state| {
                state.on_left_button_up(x, y)
            });
            return LRESULT(0);
        }
        WM_DROPFILES => {
            with_state_mut(state_ptr, "handle dropped files", (), |state| {
                state.on_drop_files(HDROP(wparam.0 as *mut c_void))
            });
            return LRESULT(0);
        }
        WM_RBUTTONUP | WM_CONTEXTMENU => {
            handle_tray_menu_command(hwnd);
            return LRESULT(0);
        }
        WM_CHAR => {
            if let Some(ch) = char::from_u32(wparam.0 as u32) {
                with_state_mut(state_ptr, "handle typed character", (), |state| {
                    state.on_char(ch)
                });
            }
            return LRESULT(0);
        }
        WM_PAINT => {
            with_state_mut(state_ptr, "paint overlay", (), |state| state.paint());
            return LRESULT(0);
        }
        WM_TIMER => {
            if wparam.0 == SPLASH_TIMER_ID {
                with_state_mut(state_ptr, "hide splash", (), |state| {
                    if state.splash_visible {
                        state.hide();
                    }
                });
                return LRESULT(0);
            }
            if wparam.0 == CLOSE_REFRESH_TIMER_ID {
                with_state_mut(state_ptr, "poll pending close windows", (), |state| {
                    state.poll_pending_close_windows()
                });
                return LRESULT(0);
            }
            if wparam.0 == APP_SCAN_TIMER_ID {
                with_state_mut(state_ptr, "poll app scan", (), |state| {
                    state.poll_app_scan()
                });
                return LRESULT(0);
            }
            if wparam.0 == HIGHLIGHT_TIMER_ID {
                with_state_mut(state_ptr, "hide attention border", (), |state| {
                    state.hide_attention_border()
                });
                return LRESULT(0);
            }
            if wparam.0 == UPDATE_CHECK_TIMER_ID {
                with_state_mut(state_ptr, "poll update check", (), |state| {
                    state.poll_update_check()
                });
                return LRESULT(0);
            }
        }
        WM_DESTROY => {
            let state = Rc::from_raw(state_ptr);
            {
                let mut state = state.borrow_mut();
                state.remove_tray_icon();
                state.unregister_thumbnails();
            }
            PostQuitMessage(0);
            return LRESULT(0);
        }
        _ => {}
    }

    DefWindowProcW(hwnd, message, wparam, lparam)
}

unsafe fn handle_tray_menu_command(hwnd: HWND) {
    match show_context_menu(hwnd, is_run_at_startup_enabled()) {
        TrayMenuCommand::None => {}
        TrayMenuCommand::ToggleStartup => {
            let enabled = is_run_at_startup_enabled();
            if !enabled {
                prompt_to_remove_legacy_startup_entries(hwnd);
            }
            if !set_run_at_startup(!enabled) {
                log_runtime_issue("Failed to update the Windows startup registry value.");
            }
        }
        TrayMenuCommand::Exit => {
            let _ = DestroyWindow(hwnd);
        }
    }
}

unsafe fn prompt_to_remove_legacy_startup_entries(hwnd: HWND) {
    let entries = legacy_startup_entries();
    if entries.is_empty() {
        return;
    }

    if !confirm_remove_legacy_startup_entries(hwnd, &entries) {
        return;
    }

    if !remove_startup_entries(&entries) {
        log_runtime_issue("Failed to remove one or more older Windows startup registry values.");
    }
}

unsafe fn confirm_remove_legacy_startup_entries(
    hwnd: HWND,
    entries: &[LegacyStartupEntry],
) -> bool {
    let title = to_wide_null("Mega Win Alt Tab startup cleanup");
    let message = to_wide_null(&legacy_startup_prompt(entries));
    MessageBoxW(
        hwnd,
        PCWSTR(message.as_ptr()),
        PCWSTR(title.as_ptr()),
        MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
    ) == IDYES
}

fn legacy_startup_prompt(entries: &[LegacyStartupEntry]) -> String {
    const MAX_PROMPT_ENTRIES: usize = 8;
    let mut prompt = String::from(
        "Mega Win Alt Tab found possible older startup entries created by earlier release file names.\n\n",
    );
    prompt.push_str("Remove these old entries before setting this copy to run at startup?\n\n");

    for entry in entries.iter().take(MAX_PROMPT_ENTRIES) {
        prompt.push_str(&format!(
            "Name: {}\nPath: {}\n\n",
            entry.name, entry.command
        ));
    }

    if entries.len() > MAX_PROMPT_ENTRIES {
        prompt.push_str(&format!(
            "...and {} more possible older startup entries.\n\n",
            entries.len() - MAX_PROMPT_ENTRIES
        ));
    }

    prompt.push_str("Yes removes the listed entries and sets this copy to run at startup.\n");
    prompt.push_str("No leaves them alone and still sets this copy to run at startup.");
    prompt
}

unsafe fn with_state_mut<T>(
    state_ptr: *const RefCell<AppState>,
    context: &str,
    fallback: T,
    action: impl FnOnce(&mut AppState) -> T,
) -> T {
    let state = &*state_ptr;
    let Ok(mut state) = state.try_borrow_mut() else {
        log_runtime_issue(&format!("Skipped {context}: app state was already busy."));
        return fallback;
    };

    action(&mut state)
}

fn log_runtime_issue(message: &str) {
    let dir = runtime_log_dir();
    if create_dir_all(&dir).is_err() {
        return;
    }

    let path = dir.join("mega-win-alt-tab.log");
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };

    let _ = writeln!(file, "{:?} {}", SystemTime::now(), message);
}

fn runtime_log_dir() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("MegaWinAltTab")
}

unsafe fn enumerate_windows(self_hwnd: HWND, include_all_desktops: bool) -> Vec<WindowEntry> {
    struct EnumContext {
        self_hwnd: HWND,
        include_all_desktops: bool,
        virtual_desktop_manager: Option<IVirtualDesktopManager>,
        monitor_numbers: HashMap<isize, u32>,
        windows: Vec<WindowEntry>,
    }

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let context = &mut *(lparam.0 as *mut EnumContext);
            if let Some(entry) = inspect_window(
                hwnd,
                context.self_hwnd,
                context.include_all_desktops,
                context.virtual_desktop_manager.as_ref(),
                &context.monitor_numbers,
            ) {
                context.windows.push(entry);
            }
        }));
        if result.is_err() {
            log_runtime_issue("Recovered from a panic while enumerating windows.");
        }
        BOOL(1)
    }

    let mut context = EnumContext {
        self_hwnd,
        include_all_desktops,
        virtual_desktop_manager: virtual_desktop_manager(),
        monitor_numbers: enumerate_monitor_numbers(),
        windows: Vec::new(),
    };
    EnumWindows(
        Some(callback),
        LPARAM(&mut context as *mut EnumContext as isize),
    )
    .ok();
    context.windows
}

unsafe fn inspect_window(
    hwnd: HWND,
    self_hwnd: HWND,
    include_all_desktops: bool,
    virtual_desktop_manager: Option<&IVirtualDesktopManager>,
    monitor_numbers: &HashMap<isize, u32>,
) -> Option<WindowEntry> {
    if hwnd == self_hwnd || hwnd == GetShellWindow() {
        return None;
    }
    if !IsWindowVisible(hwnd).as_bool() {
        return None;
    }
    if is_tool_window(hwnd) {
        return None;
    }

    let desktop_location = window_desktop_location(virtual_desktop_manager, hwnd);
    let cloaked = is_cloaked(hwnd);
    if !should_include_window_for_desktop(include_all_desktops, desktop_location, cloaked) {
        return None;
    }

    let title = get_window_text(hwnd);
    if title.trim().is_empty() {
        return None;
    }

    let class_name = get_class_name(hwnd);
    let app_name = get_process_name(hwnd);

    Some(WindowEntry {
        hwnd: hwnd_to_isize(hwnd),
        title,
        app_name,
        class_name,
        screen_number: window_screen_number(hwnd, monitor_numbers),
        desktop_location,
        minimized: IsIconic(hwnd).as_bool(),
        has_thumbnail: !cloaked,
    })
}

unsafe fn is_tool_window(hwnd: HWND) -> bool {
    let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    ex_style & WS_EX_TOOLWINDOW.0 != 0
}

unsafe fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut u32 as *mut c_void,
        size_of::<u32>() as u32,
    )
    .is_ok()
        && cloaked != 0
}

unsafe fn pending_close_window_still_listable(
    hwnd: HWND,
    self_hwnd: HWND,
    include_all_desktops: bool,
    virtual_desktop_manager: Option<&IVirtualDesktopManager>,
) -> bool {
    if hwnd == self_hwnd || !IsWindow(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
        return false;
    }
    if is_tool_window(hwnd) {
        return false;
    }

    let desktop_location = window_desktop_location(virtual_desktop_manager, hwnd);
    let cloaked = is_cloaked(hwnd);
    if !should_include_window_for_desktop(include_all_desktops, desktop_location, cloaked) {
        return false;
    }

    !get_window_text(hwnd).trim().is_empty()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingClosePollDecision {
    Keep(u8),
    RemoveAndRefresh,
    RemoveQuietly,
}

fn pending_close_poll_decision(
    window_still_listable: bool,
    attempts_remaining: u8,
) -> PendingClosePollDecision {
    if !window_still_listable {
        return PendingClosePollDecision::RemoveAndRefresh;
    }
    if attempts_remaining == 0 {
        return PendingClosePollDecision::RemoveQuietly;
    }

    PendingClosePollDecision::Keep(attempts_remaining - 1)
}

unsafe fn highlight_rect_for_window(hwnd: HWND) -> Option<RECT> {
    extended_frame_bounds(hwnd)
        .or_else(|| current_window_rect(hwnd))
        .filter(|rect| rect_is_usable(*rect))
        .or_else(|| monitor_rect_for_window(hwnd))
}

unsafe fn extended_frame_bounds(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    DwmGetWindowAttribute(
        hwnd,
        DWMWA_EXTENDED_FRAME_BOUNDS,
        &mut rect as *mut RECT as *mut c_void,
        size_of::<RECT>() as u32,
    )
    .is_ok()
    .then_some(rect)
    .filter(|rect| rect_is_usable(*rect))
}

fn attention_border_positions(rect: RECT, bounds: RECT, thickness: i32) -> [RECT; 4] {
    let thickness = thickness.max(1);
    let bounds = if rect_is_usable(bounds) { bounds } else { rect };
    let (horizontal_left, horizontal_right) = bounded_span(
        rect.left - thickness,
        rect.right + thickness,
        bounds.left,
        bounds.right,
        thickness,
    );
    let (vertical_top, vertical_bottom) =
        bounded_span(rect.top, rect.bottom, bounds.top, bounds.bottom, thickness);
    let top = clamped_strip_start(rect.top - thickness, bounds.top, bounds.bottom, thickness);
    let bottom = if rect.bottom + thickness > bounds.bottom {
        clamped_strip_start(
            rect.bottom - thickness,
            bounds.top,
            bounds.bottom,
            thickness,
        )
    } else {
        clamped_strip_start(rect.bottom, bounds.top, bounds.bottom, thickness)
    };
    let left = clamped_strip_start(rect.left - thickness, bounds.left, bounds.right, thickness);
    let right = if rect.right + thickness > bounds.right {
        clamped_strip_start(rect.right - thickness, bounds.left, bounds.right, thickness)
    } else {
        clamped_strip_start(rect.right, bounds.left, bounds.right, thickness)
    };

    [
        RECT {
            left: horizontal_left,
            top,
            right: horizontal_right,
            bottom: top + thickness,
        },
        RECT {
            left: horizontal_left,
            top: bottom,
            right: horizontal_right,
            bottom: bottom + thickness,
        },
        RECT {
            left,
            top: vertical_top,
            right: left + thickness,
            bottom: vertical_bottom,
        },
        RECT {
            left: right,
            top: vertical_top,
            right: right + thickness,
            bottom: vertical_bottom,
        },
    ]
}

fn bounded_span(start: i32, end: i32, min: i32, max: i32, minimum_size: i32) -> (i32, i32) {
    let minimum_size = minimum_size.max(1);
    if max <= min + minimum_size {
        return (min, max.max(min + 1));
    }

    let start = start.clamp(min, max);
    let end = end.clamp(min, max);
    if end - start >= minimum_size {
        return (start, end);
    }

    let end = (start + minimum_size).min(max);
    let start = (end - minimum_size).max(min);
    (start, end)
}

fn clamped_strip_start(value: i32, min: i32, max: i32, size: i32) -> i32 {
    let size = size.max(1);
    value.clamp(min, (max - size).max(min))
}

fn rect_is_usable(rect: RECT) -> bool {
    rect.right > rect.left && rect.bottom > rect.top
}

unsafe fn get_window_text(hwnd: HWND) -> String {
    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; len as usize + 1];
    let read = GetWindowTextW(hwnd, &mut buffer);
    String::from_utf16_lossy(&buffer[..read as usize])
}

unsafe fn get_class_name(hwnd: HWND) -> String {
    let mut buffer = vec![0u16; 256];
    let read = GetClassNameW(hwnd, &mut buffer);
    if read == 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..read as usize])
}

unsafe fn get_process_name(hwnd: HWND) -> String {
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return String::new();
    }

    let Ok(process) = OpenProcess(
        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
        BOOL(0),
        pid,
    ) else {
        return String::new();
    };

    let name = query_process_name(process);
    let _ = CloseHandle(process);
    name
}

unsafe fn query_process_name(process: HANDLE) -> String {
    let mut buffer = vec![0u16; 32768];
    let mut len = buffer.len() as u32;
    if QueryFullProcessImageNameW(
        process,
        PROCESS_NAME_FORMAT(0),
        PWSTR(buffer.as_mut_ptr()),
        &mut len,
    )
    .is_err()
    {
        return String::new();
    }
    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    Path::new(&path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(&path)
        .to_string()
}

unsafe fn scan_chrome_tabs(windows: &[WindowEntry]) -> Vec<TabEntry> {
    let Ok(automation) =
        CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
    else {
        return Vec::new();
    };

    let mut tabs = Vec::new();
    let now = Instant::now();
    for window in windows
        .iter()
        .filter(|window| window.class_name == "Chrome_WidgetWin_1")
    {
        tabs.extend(scan_tabs_for_window(&automation, window, now));
    }
    tabs
}

unsafe fn scan_tabs_for_window(
    automation: &IUIAutomation,
    window: &WindowEntry,
    now: Instant,
) -> Vec<TabEntry> {
    let Ok(root) = automation.ElementFromHandle(hwnd_from_isize(window.hwnd)) else {
        return Vec::new();
    };

    let condition = match create_control_type_condition(automation, UIA_TabItemControlTypeId.0) {
        Some(condition) => condition,
        None => return Vec::new(),
    };

    let Ok(collection) = root.FindAll(TreeScope_Descendants, &condition) else {
        return Vec::new();
    };

    let Ok(length) = collection.Length() else {
        return Vec::new();
    };

    let mut tabs = Vec::new();
    let mut seen = HashSet::new();
    for index in 0..length {
        let Ok(element) = collection.GetElement(index) else {
            continue;
        };
        let title = get_uia_name(&element);
        if title.trim().is_empty() || title.eq_ignore_ascii_case("new tab") {
            continue;
        }
        if !seen.insert(title.to_lowercase()) {
            continue;
        }
        tabs.push(TabEntry {
            browser: "chrome".to_string(),
            parent_hwnd: Some(window.hwnd),
            window_title: Some(window.title.clone()),
            title,
            active: false,
            extension_window_id: None,
            extension_tab_id: None,
            source: TabSource::Accessibility,
            last_seen: now,
        });
    }
    tabs
}

unsafe fn select_chrome_tab_by_title(hwnd: HWND, title: &str) -> bool {
    for attempt in 0..5 {
        if attempt > 0 {
            sleep(Duration::from_millis(75));
        }
        if select_chrome_tab_by_title_once(hwnd, title) {
            return true;
        }
    }

    false
}

unsafe fn select_chrome_tab_by_title_once(hwnd: HWND, title: &str) -> bool {
    let Ok(automation) =
        CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
    else {
        return false;
    };

    let Ok(root) = automation.ElementFromHandle(hwnd) else {
        return false;
    };

    let Some(condition) = create_control_type_condition(&automation, UIA_TabItemControlTypeId.0)
    else {
        return false;
    };

    let Ok(collection) = root.FindAll(TreeScope_Descendants, &condition) else {
        return false;
    };

    let Ok(length) = collection.Length() else {
        return false;
    };

    for index in 0..length {
        let Ok(element) = collection.GetElement(index) else {
            continue;
        };
        let accessible_title = get_uia_name(&element);
        if !chrome_tab_title_matches(&accessible_title, title) {
            continue;
        }
        if activate_chrome_tab_element(&element) {
            return true;
        }
    }

    false
}

unsafe fn activate_chrome_tab_element(element: &IUIAutomationElement) -> bool {
    let _ = element.SetFocus();

    if let Ok(pattern) =
        element.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId)
    {
        if pattern.Select().is_ok() {
            let _ = element.SetFocus();
            return true;
        }
    }

    if let Ok(pattern) =
        element.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
    {
        if pattern.Invoke().is_ok() {
            let _ = element.SetFocus();
            return true;
        }
    }

    element.SetFocus().is_ok()
}

fn chrome_tab_title_matches(accessible_title: &str, requested_title: &str) -> bool {
    let accessible = normalize_chrome_tab_title(accessible_title);
    let requested = normalize_chrome_tab_title(requested_title);

    !requested.is_empty()
        && (titles_match(&accessible, &requested) || titles_match(&requested, &accessible))
}

fn normalize_chrome_tab_title(title: &str) -> String {
    let normalized = normalize_for_match(title);
    strip_title_suffix(&normalized, &[" - google chrome", " - chrome"]).to_string()
}

fn strip_title_suffix<'a>(title: &'a str, suffixes: &[&str]) -> &'a str {
    suffixes
        .iter()
        .find_map(|suffix| title.strip_suffix(suffix))
        .unwrap_or(title)
        .trim()
}

fn titles_match(value: &str, prefix: &str) -> bool {
    if value == prefix {
        return true;
    }

    value
        .strip_prefix(prefix)
        .is_some_and(title_prefix_boundary)
}

fn title_prefix_boundary(rest: &str) -> bool {
    rest.is_empty()
        || rest.starts_with(" - ")
        || rest.starts_with(" | ")
        || rest.starts_with(": ")
        || rest.starts_with(" – ")
        || rest.starts_with(" — ")
}

unsafe fn create_control_type_condition(
    automation: &IUIAutomation,
    control_type: i32,
) -> Option<IUIAutomationCondition> {
    let value = VARIANT::from(control_type);
    automation
        .CreatePropertyCondition(UIA_ControlTypePropertyId, &value)
        .ok()
}

unsafe fn get_uia_name(element: &IUIAutomationElement) -> String {
    if let Ok(name) = element.CurrentName() {
        return name.to_string();
    }
    String::new()
}

unsafe fn activate_window(hwnd: HWND) {
    if IsIconic(hwnd).as_bool() {
        let _ = ShowWindow(hwnd, SW_RESTORE);
    }
    let _ = SetForegroundWindow(hwnd);
}

unsafe fn activate_window_from_overlay(hwnd: HWND, overlay_hwnd: HWND) {
    move_window_to_overlay_desktop_if_needed(hwnd, overlay_hwnd);
    activate_window(hwnd);
}

unsafe fn run_activation(request: ActivationRequest) -> Option<isize> {
    match request.result.target {
        ActivationTarget::Window { hwnd } => {
            activate_window_from_overlay(hwnd_from_isize(hwnd), request.overlay_hwnd);
        }
        ActivationTarget::Tab {
            parent_hwnd,
            ref browser,
            ref title,
            extension_window_id,
            extension_tab_id,
        } => {
            if let (Some(window_id), Some(tab_id)) = (extension_window_id, extension_tab_id) {
                request
                    .bridge
                    .queue_activate_tab(browser, window_id, tab_id);
            }

            let parent = if let Some(hwnd) = parent_hwnd {
                Some(hwnd_from_isize(hwnd))
            } else {
                request
                    .windows
                    .iter()
                    .find(|window| window.class_name == "Chrome_WidgetWin_1")
                    .map(|chrome| hwnd_from_isize(chrome.hwnd))
            };

            if let Some(parent) = parent {
                activate_window_from_overlay(parent, request.overlay_hwnd);
                if browser == "chrome" {
                    let _ = select_chrome_tab_by_title(parent, title);
                }
            }
        }
        ActivationTarget::App { ref launch_path } => {
            let _ = launch_app(request.overlay_hwnd, launch_path);
        }
        ActivationTarget::Folder { ref path } => {
            let _ = open_folder(request.overlay_hwnd, path);
        }
    }

    if request.restore_overlay_focus {
        restore_overlay_focus(request.overlay_hwnd);
    }
    request.highlight_target
}

unsafe fn run_close_window(request: CloseWindowRequest) -> bool {
    let hwnd = hwnd_from_isize(request.hwnd);
    if !IsWindow(hwnd).as_bool() {
        return false;
    }

    move_window_to_overlay_desktop_if_needed(hwnd, request.overlay_hwnd);
    let close_sent = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).is_ok();
    restore_overlay_focus(request.overlay_hwnd);
    close_sent
}

unsafe fn run_minimize_window(request: MinimizeWindowRequest) {
    let hwnd = hwnd_from_isize(request.hwnd);
    if !IsWindow(hwnd).as_bool() {
        return;
    }

    move_window_to_overlay_desktop_if_needed(hwnd, request.overlay_hwnd);
    let _ = ShowWindow(hwnd, SW_MINIMIZE);
    restore_overlay_focus(request.overlay_hwnd);
}

unsafe fn run_toggle_maximize(request: ToggleMaximizeRequest) {
    let hwnd = hwnd_from_isize(request.hwnd);
    if !IsWindow(hwnd).as_bool() {
        return;
    }

    move_window_to_overlay_desktop_if_needed(hwnd, request.overlay_hwnd);
    let show_command = if IsZoomed(hwnd).as_bool() {
        SW_RESTORE
    } else {
        SW_MAXIMIZE
    };
    let _ = ShowWindow(hwnd, show_command);
    restore_overlay_focus(request.overlay_hwnd);
}

unsafe fn run_move_to_monitor(request: MoveWindowRequest) -> bool {
    let hwnd = hwnd_from_isize(request.hwnd);
    if !IsWindow(hwnd).as_bool() {
        return false;
    }

    move_window_to_overlay_desktop_if_needed(hwnd, request.overlay_hwnd);

    let moved = move_window_to_monitor(hwnd, request.original_rect, request.direction);
    if moved {
        restore_overlay_focus(request.overlay_hwnd);
    }
    moved
}

unsafe fn run_peek_window(request: PeekWindowRequest) -> Option<isize> {
    let hwnd = hwnd_from_isize(request.hwnd);
    if !IsWindow(hwnd).as_bool() {
        restore_overlay_focus(request.overlay_hwnd);
        return None;
    }

    move_window_to_overlay_desktop_if_needed(hwnd, request.overlay_hwnd);
    show_window_without_activation(hwnd);
    restore_overlay_focus(request.overlay_hwnd);
    request.highlight_target
}

unsafe fn show_window_without_activation(hwnd: HWND) {
    if IsIconic(hwnd).as_bool() {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    let _ = SetWindowPos(
        hwnd,
        HWND_TOP,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    );
}

fn selected_move_target_hwnd(result: &SearchResult, windows: &[WindowEntry]) -> Option<isize> {
    match &result.target {
        ActivationTarget::Window { hwnd } => Some(*hwnd),
        ActivationTarget::Tab { parent_hwnd, .. } => *parent_hwnd,
        ActivationTarget::App { .. } => running_window_for_app(&result.title, windows),
        ActivationTarget::Folder { .. } => None,
    }
}

fn selected_folder_removal_target(
    mode: OverlayMode,
    result: Option<&SearchResult>,
) -> Option<String> {
    if mode != OverlayMode::Apps {
        return None;
    }

    match result.map(|result| &result.target) {
        Some(ActivationTarget::Folder { path }) => Some(path.clone()),
        _ => None,
    }
}

fn should_highlight_repeated_peek(
    last_target: Option<isize>,
    last_at: Option<Instant>,
    target: Option<isize>,
    now: Instant,
) -> bool {
    let (Some(last_target), Some(last_at), Some(target)) = (last_target, last_at, target) else {
        return false;
    };

    last_target == target
        && now.duration_since(last_at) <= Duration::from_millis(HIGHLIGHT_SECOND_TAP_MS)
}

fn running_window_for_app(app_title: &str, windows: &[WindowEntry]) -> Option<isize> {
    windows
        .iter()
        .find(|window| app_title_matches_window(app_title, window))
        .map(|window| window.hwnd)
}

fn app_title_matches_window(app_title: &str, window: &WindowEntry) -> bool {
    let app_title = normalize_window_match_text(app_title);
    if app_title.is_empty() {
        return false;
    }

    let app_name = normalize_window_match_text(&window.app_name);
    let title = normalize_window_match_text(&window.title);

    app_name == app_title
        || title == app_title
        || title.starts_with(&(app_title.clone() + " "))
        || title.ends_with(&(" ".to_string() + &app_title))
}

fn normalize_window_match_text(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .map(|ch| if ch.is_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

unsafe fn restore_overlay_focus(hwnd: HWND) {
    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
    let _ = SetForegroundWindow(hwnd);
    let _ = SetFocus(hwnd);
    let _ = InvalidateRect(hwnd, None, BOOL(1));
}

unsafe fn open_github_releases(hwnd: HWND) -> bool {
    let result = ShellExecuteW(
        hwnd,
        w!("open"),
        GITHUB_RELEASES_URL,
        PCWSTR::null(),
        PCWSTR::null(),
        SW_SHOWNORMAL,
    );
    result.0 as isize > 32
}

fn update_link_text(latest_version: &str) -> String {
    format!("Update available: {latest_version} - open releases")
}

fn result_thumbnail_hwnd(result: &SearchResult) -> Option<isize> {
    match &result.target {
        ActivationTarget::Window { hwnd } => Some(*hwnd),
        ActivationTarget::Tab { parent_hwnd, .. } => *parent_hwnd,
        ActivationTarget::App { .. } | ActivationTarget::Folder { .. } => None,
    }
}

fn is_app_mode_toggle_key(key: u32) -> bool {
    key == VK_OEM_2.0 as u32
}

unsafe fn fill_rect(hdc: HDC, rect: RECT, color: COLORREF) {
    let brush = CreateSolidBrush(color);
    FillRect(hdc, &rect, HBRUSH(brush.0));
    let _ = DeleteObject(brush);
}

#[derive(Clone, Copy)]
struct SearchButtonRects {
    help: RECT,
    close: RECT,
}

fn search_button_rects(search_rect: RECT) -> SearchButtonRects {
    let close = RECT {
        left: search_rect.right - 46,
        top: search_rect.top + 14,
        right: search_rect.right - 18,
        bottom: search_rect.top + 42,
    };
    let help = RECT {
        left: close.left - 38,
        top: close.top,
        right: close.left - 10,
        bottom: close.bottom,
    };

    SearchButtonRects { help, close }
}

unsafe fn draw_rect_outline(hdc: HDC, rect: RECT, color: COLORREF) {
    let pen = CreatePen(PS_SOLID, 1, color);
    let old_pen = SelectObject(hdc, pen);
    let _ = MoveToEx(hdc, rect.left, rect.top, None);
    let _ = LineTo(hdc, rect.right - 1, rect.top);
    let _ = LineTo(hdc, rect.right - 1, rect.bottom - 1);
    let _ = LineTo(hdc, rect.left, rect.bottom - 1);
    let _ = LineTo(hdc, rect.left, rect.top);
    SelectObject(hdc, old_pen);
    let _ = DeleteObject(pen);
}

unsafe fn draw_help_button(hdc: HDC, rect: RECT, hovered: bool) {
    let brush = CreateSolidBrush(if hovered {
        rgb(82, 112, 150)
    } else {
        rgb(57, 63, 70)
    });
    let pen = CreatePen(
        PS_SOLID,
        1,
        if hovered {
            rgb(172, 205, 242)
        } else {
            rgb(120, 130, 141)
        },
    );
    let old_brush = SelectObject(hdc, brush);
    let old_pen = SelectObject(hdc, pen);
    let _ = Ellipse(hdc, rect.left, rect.top, rect.right, rect.bottom);
    SelectObject(hdc, old_pen);
    SelectObject(hdc, old_brush);
    let _ = DeleteObject(pen);
    let _ = DeleteObject(brush);

    let font = make_font(17, FW_BOLD.0 as i32);
    let old_font = SelectObject(hdc, font);
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, rgb(248, 250, 252));
    draw_text(
        hdc,
        "?",
        rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
    SelectObject(hdc, old_font);
    let _ = DeleteObject(font);
}

unsafe fn draw_close_button(hdc: HDC, rect: RECT, hovered: bool) {
    let fill = if hovered {
        rgb(142, 56, 66)
    } else {
        rgb(57, 63, 70)
    };
    let outline = if hovered {
        rgb(242, 176, 184)
    } else {
        rgb(120, 130, 141)
    };

    fill_rect(hdc, rect, fill);
    draw_rect_outline(hdc, rect, outline);

    let font = make_font(17, FW_BOLD.0 as i32);
    let old_font = SelectObject(hdc, font);
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, rgb(248, 250, 252));
    draw_text(
        hdc,
        "x",
        rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
    SelectObject(hdc, old_font);
    let _ = DeleteObject(font);
}

unsafe fn make_font(height: i32, weight: i32) -> HFONT {
    CreateFontW(
        -height,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        w!("Segoe UI"),
    )
}

unsafe fn draw_text(hdc: HDC, text: &str, mut rect: RECT, format: DRAW_TEXT_FORMAT) {
    let mut wide = to_wide(text);
    DrawTextW(hdc, &mut wide, &mut rect, format);
}

fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

fn to_wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn hwnd_from_isize(value: isize) -> HWND {
    HWND(value as *mut c_void)
}

fn hwnd_to_isize(hwnd: HWND) -> isize {
    hwnd.0 as isize
}

unsafe fn is_control_down() -> bool {
    GetKeyState(VK_CONTROL.0 as i32) < 0
}

fn visible_result_count(
    rect: RECT,
    top: i32,
    row_height: i32,
    gap: i32,
    result_count: usize,
) -> usize {
    if result_count == 0 {
        return 0;
    }

    let available_height = (rect.bottom - top).max(row_height);
    let rows = ((available_height + gap) / (row_height + gap)).max(1) as usize;
    rows.min(MAX_RESULTS).min(result_count)
}

fn visible_result_start(selected: usize, visible_rows: usize) -> usize {
    if visible_rows == 0 {
        0
    } else {
        selected.saturating_add(1).saturating_sub(visible_rows)
    }
}

fn selection_status_text(selected: usize, total: usize) -> String {
    if total == 0 {
        "0 / 0".to_string()
    } else {
        format!("{} / {}", selected.min(total - 1) + 1, total)
    }
}

fn result_number_label(index: usize) -> String {
    (index + 1).to_string()
}

fn result_number_column_width(total: usize) -> i32 {
    let digits = total.max(1).to_string().len() as i32;
    (digits * 12 + 24).max(38)
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | ((g as u32) << 8) | ((b as u32) << 16))
}

fn to_win_error(error: anyhow::Error) -> windows::core::Error {
    windows::core::Error::new(
        windows::core::HRESULT(0x80004005u32 as i32),
        error.to_string(),
    )
}

fn has_startup_arg<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .any(|arg| arg.as_ref() == OsStr::new("--startup"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mega_win_alt_tab::core::DesktopLocation;

    fn result_with_target(target: ActivationTarget) -> SearchResult {
        SearchResult {
            kind: SearchResultKind::Window,
            title: "Result".to_string(),
            subtitle: "Subtitle".to_string(),
            screen_number: None,
            rank: 0,
            target,
        }
    }

    fn window_entry(hwnd: isize, title: &str, app_name: &str) -> WindowEntry {
        WindowEntry {
            hwnd,
            title: title.to_string(),
            app_name: app_name.to_string(),
            class_name: "TestWindow".to_string(),
            screen_number: Some(1),
            desktop_location: DesktopLocation::Current,
            minimized: false,
            has_thumbnail: true,
        }
    }

    #[test]
    fn result_thumbnail_handles_windows_tabs_and_apps() {
        assert_eq!(
            result_thumbnail_hwnd(&result_with_target(ActivationTarget::Window { hwnd: 42 })),
            Some(42)
        );
        assert_eq!(
            result_thumbnail_hwnd(&result_with_target(ActivationTarget::Tab {
                parent_hwnd: Some(77),
                browser: "chrome".to_string(),
                title: "Tab".to_string(),
                extension_window_id: None,
                extension_tab_id: None,
            })),
            Some(77)
        );
        assert_eq!(
            result_thumbnail_hwnd(&result_with_target(ActivationTarget::Tab {
                parent_hwnd: None,
                browser: "chrome".to_string(),
                title: "Tab".to_string(),
                extension_window_id: Some(1),
                extension_tab_id: Some(2),
            })),
            None
        );
        assert_eq!(
            result_thumbnail_hwnd(&result_with_target(ActivationTarget::App {
                launch_path: "C:\\Tools\\Signal.lnk".to_string(),
            })),
            None
        );
        assert_eq!(
            result_thumbnail_hwnd(&result_with_target(ActivationTarget::Folder {
                path: "C:\\Users\\Example\\Downloads".to_string(),
            })),
            None
        );
    }

    #[test]
    fn selected_move_target_uses_window_or_tab_parent_handles() {
        assert_eq!(
            selected_move_target_hwnd(
                &result_with_target(ActivationTarget::Window { hwnd: 42 }),
                &[]
            ),
            Some(42)
        );
        assert_eq!(
            selected_move_target_hwnd(
                &result_with_target(ActivationTarget::Tab {
                    parent_hwnd: Some(77),
                    browser: "chrome".to_string(),
                    title: "Tab".to_string(),
                    extension_window_id: None,
                    extension_tab_id: None,
                }),
                &[]
            ),
            Some(77)
        );
    }

    #[test]
    fn selected_move_target_matches_running_app_results_when_possible() {
        let windows = vec![
            window_entry(5, "Notes", "notepad"),
            window_entry(9, "Signal", "Signal"),
        ];
        let mut result = result_with_target(ActivationTarget::App {
            launch_path: "C:\\Users\\Example\\Signal.lnk".to_string(),
        });
        result.title = "Signal".to_string();

        assert_eq!(selected_move_target_hwnd(&result, &windows), Some(9));
    }

    #[test]
    fn selected_move_target_has_no_window_for_non_running_app_results() {
        let windows = vec![window_entry(5, "Notes", "notepad")];
        let mut result = result_with_target(ActivationTarget::App {
            launch_path: "C:\\Users\\Example\\Signal.lnk".to_string(),
        });
        result.title = "Signal".to_string();

        assert_eq!(selected_move_target_hwnd(&result, &windows), None);
    }

    #[test]
    fn selected_folder_removal_target_only_applies_in_app_mode() {
        let result = result_with_target(ActivationTarget::Folder {
            path: "C:\\Users\\Example\\Downloads".to_string(),
        });

        assert_eq!(
            selected_folder_removal_target(OverlayMode::Apps, Some(&result)),
            Some("C:\\Users\\Example\\Downloads".to_string())
        );
        assert_eq!(
            selected_folder_removal_target(OverlayMode::WindowsAndTabs, Some(&result)),
            None
        );
        assert_eq!(
            selected_folder_removal_target(OverlayMode::Apps, None),
            None
        );
    }

    #[test]
    fn repeated_peek_highlights_same_target_within_short_window() {
        let first_tap = Instant::now();
        let second_tap = first_tap + Duration::from_millis(HIGHLIGHT_SECOND_TAP_MS - 1);
        let late_tap = first_tap + Duration::from_millis(HIGHLIGHT_SECOND_TAP_MS + 1);

        assert!(should_highlight_repeated_peek(
            Some(42),
            Some(first_tap),
            Some(42),
            second_tap
        ));
        assert!(!should_highlight_repeated_peek(
            Some(42),
            Some(first_tap),
            Some(77),
            second_tap
        ));
        assert!(!should_highlight_repeated_peek(
            Some(42),
            Some(first_tap),
            Some(42),
            late_tap
        ));
        assert!(!should_highlight_repeated_peek(
            None,
            Some(first_tap),
            Some(42),
            second_tap
        ));
    }

    #[test]
    fn pending_close_poll_refreshes_when_window_is_no_longer_listable() {
        assert_eq!(
            pending_close_poll_decision(false, CLOSE_REFRESH_ATTEMPTS),
            PendingClosePollDecision::RemoveAndRefresh
        );
    }

    #[test]
    fn pending_close_poll_keeps_visible_window_until_attempts_expire() {
        assert_eq!(
            pending_close_poll_decision(true, 2),
            PendingClosePollDecision::Keep(1)
        );
        assert_eq!(
            pending_close_poll_decision(true, 0),
            PendingClosePollDecision::RemoveQuietly
        );
    }

    #[test]
    fn attention_border_stays_outside_normal_window_when_space_allows() {
        let rect = RECT {
            left: 100,
            top: 100,
            right: 500,
            bottom: 400,
        };
        let bounds = RECT {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 800,
        };

        assert_eq!(
            attention_border_positions(rect, bounds, 6),
            [
                RECT {
                    left: 94,
                    top: 94,
                    right: 506,
                    bottom: 100,
                },
                RECT {
                    left: 94,
                    top: 400,
                    right: 506,
                    bottom: 406,
                },
                RECT {
                    left: 94,
                    top: 100,
                    right: 100,
                    bottom: 400,
                },
                RECT {
                    left: 500,
                    top: 100,
                    right: 506,
                    bottom: 400,
                },
            ]
        );
    }

    #[test]
    fn attention_border_moves_inside_maximized_window_bounds() {
        let rect = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };

        assert_eq!(
            attention_border_positions(rect, rect, 6),
            [
                RECT {
                    left: 0,
                    top: 0,
                    right: 1920,
                    bottom: 6,
                },
                RECT {
                    left: 0,
                    top: 1074,
                    right: 1920,
                    bottom: 1080,
                },
                RECT {
                    left: 0,
                    top: 0,
                    right: 6,
                    bottom: 1080,
                },
                RECT {
                    left: 1914,
                    top: 0,
                    right: 1920,
                    bottom: 1080,
                },
            ]
        );
    }

    #[test]
    fn update_link_text_names_version_and_destination() {
        assert_eq!(
            update_link_text("v1.2.3"),
            "Update available: v1.2.3 - open releases"
        );
    }

    #[test]
    fn selection_status_is_one_based_and_clamped() {
        assert_eq!(selection_status_text(0, 0), "0 / 0");
        assert_eq!(selection_status_text(0, 20), "1 / 20");
        assert_eq!(selection_status_text(19, 20), "20 / 20");
        assert_eq!(selection_status_text(99, 20), "20 / 20");
    }

    #[test]
    fn result_number_labels_are_one_based() {
        assert_eq!(result_number_label(0), "1");
        assert_eq!(result_number_label(19), "20");
    }

    #[test]
    fn result_number_column_width_expands_for_large_lists() {
        assert!(result_number_column_width(1) >= 38);
        assert!(result_number_column_width(100) > result_number_column_width(20));
    }

    #[test]
    fn search_buttons_keep_help_left_of_close() {
        let search_rect = RECT {
            left: 28,
            top: 22,
            right: 772,
            bottom: 78,
        };

        let buttons = search_button_rects(search_rect);

        assert!(buttons.help.left > search_rect.left);
        assert!(buttons.help.right < buttons.close.left);
        assert_eq!(buttons.help.top, buttons.close.top);
        assert_eq!(buttons.help.bottom, buttons.close.bottom);
        assert_eq!(buttons.close.right, search_rect.right - 18);
    }

    #[test]
    fn startup_argument_is_detected_exactly() {
        assert!(has_startup_arg(["mega-win-alt-tab.exe", "--startup"]));
        assert!(!has_startup_arg(["mega-win-alt-tab.exe"]));
        assert!(!has_startup_arg(["mega-win-alt-tab.exe", "--startup-now"]));
    }

    #[test]
    fn legacy_startup_prompt_lists_entry_names_and_paths() {
        let entries = vec![LegacyStartupEntry {
            name: "mega-win-alt-tab-v1.0.0-windows-x64.exe".to_string(),
            command: r#""C:\Downloads\mega-win-alt-tab-v1.0.0-windows-x64.exe" --startup"#
                .to_string(),
        }];

        let prompt = legacy_startup_prompt(&entries);

        assert!(prompt.contains("possible older startup entries"));
        assert!(prompt.contains("Name: mega-win-alt-tab-v1.0.0-windows-x64.exe"));
        assert!(prompt
            .contains(r#"Path: "C:\Downloads\mega-win-alt-tab-v1.0.0-windows-x64.exe" --startup"#));
        assert!(prompt.contains("No leaves them alone"));
    }

    #[test]
    fn chrome_tab_title_matching_handles_suffixes_and_separators() {
        assert!(chrome_tab_title_matches(
            "LinkedIn - Google Chrome",
            "LinkedIn"
        ));
        assert!(chrome_tab_title_matches("LinkedIn | Feed", "LinkedIn"));
        assert!(chrome_tab_title_matches("LinkedIn", "LinkedIn | Feed"));
        assert!(!chrome_tab_title_matches("Gmail", "LinkedIn"));
        assert!(!chrome_tab_title_matches("LinkedOut", "LinkedIn"));
    }
}
