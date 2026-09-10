use mega_win_alt_tab::core::{
    build_app_results, build_results_with_options, normalize_for_match, ActivationTarget, AppEntry,
    BuildResultOptions, SearchResult, SearchResultKind, TabEntry, TabSource, WindowEntry,
};
use mega_win_alt_tab::extension_bridge::ExtensionBridge;
mod apps;
mod icons;
mod input;
mod monitors;
mod startup;
mod tray;
mod virtual_desktops;

use apps::{enumerate_apps, launch_app};
use icons::create_mega_icon;
use input::{mouse_point, point_in_rect};
use monitors::{
    current_window_rect, enumerate_monitor_numbers, move_window_to_next_monitor,
    window_screen_number,
};
use startup::{is_run_at_startup_enabled, set_run_at_startup};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::c_void;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::mem::size_of;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::rc::Rc;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};
use tray::{
    install_tray_icon, is_tray_context_event, is_tray_icon_message, is_tray_select_event,
    remove_tray_icon, show_context_menu, TrayMenuCommand, WM_TRAYICON,
};
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
    DwmUpdateThumbnailProperties, DWMWA_CLOAKED, DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY,
    DWM_TNP_RECTDESTINATION, DWM_TNP_VISIBLE,
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
use windows::Win32::UI::Shell::IVirtualDesktopManager;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, DispatchMessageW, DrawIconEx,
    EnumWindows, GetClassNameW, GetClientRect, GetMessageW, GetShellWindow, GetSystemMetrics,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindow, IsWindowVisible, LoadCursorW, LoadIconW, PostQuitMessage, RegisterClassW,
    SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TranslateMessage, CS_HREDRAW, CS_VREDRAW, DI_NORMAL, GWLP_USERDATA, GWL_EXSTYLE, HICON,
    HWND_TOPMOST, ICON_BIG, ICON_SMALL, IDC_ARROW, IDI_APPLICATION, MSG, SET_WINDOW_POS_FLAGS,
    SM_CXSCREEN, SM_CYSCREEN, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_RESTORE, SW_SHOW, WM_CHAR,
    WM_CONTEXTMENU, WM_DESTROY, WM_HOTKEY, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE,
    WM_PAINT, WM_RBUTTONUP, WM_SETICON, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const HOTKEY_ID: i32 = 0x4d57;
const WM_MOUSELEAVE_MESSAGE: u32 = 0x02A3;
const MAX_RESULTS: usize = 8;
const HELP_LINES: [&str; 9] = [
    "Esc - close",
    "Up / Down - move selection",
    "Enter - select or launch",
    "Right - bring selected window to top",
    "Left - move selected window to next screen",
    "Ctrl + Right - increase thumbnails",
    "Ctrl + Left - decrease thumbnails",
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
const WINDOW_TITLE: PCWSTR = w!("Mega Win Alt Tab");

pub fn run() -> Result<()> {
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
    windows: Vec<WindowEntry>,
    accessibility_tabs: Vec<TabEntry>,
    results: Vec<SearchResult>,
    thumbnails: HashMap<isize, isize>,
    original_window_rects: HashMap<isize, RECT>,
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
            windows: Vec::new(),
            accessibility_tabs: Vec::new(),
            results: Vec::new(),
            thumbnails: HashMap::new(),
            original_window_rects: HashMap::new(),
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
        self.mode = OverlayMode::WindowsAndTabs;
        self.all_desktops = false;
        self.query.clear();
        self.selected = 0;
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

    unsafe fn hide(&mut self) {
        self.visible = false;
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
        self.unregister_thumbnails();
        let _ = ShowWindow(self.hwnd, SW_HIDE);
    }

    unsafe fn toggle(&mut self) {
        if self.visible {
            self.hide();
        } else {
            self.show();
        }
    }

    unsafe fn refresh(&mut self) {
        self.unregister_thumbnails();
        self.windows = enumerate_windows(self.hwnd, self.all_desktops);
        self.accessibility_tabs = scan_chrome_tabs(&self.windows);
        self.apps = enumerate_apps();
        self.rebuild_results();
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
            OverlayMode::Apps => build_app_results(&self.query, &self.apps),
        };
        if self.results.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.results.len() {
            self.selected = self.results.len() - 1;
        }
    }

    unsafe fn on_char(&mut self, ch: char) {
        if !self.visible {
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
        match key {
            key if key == VK_ESCAPE.0 as u32 => self.hide(),
            key if key == VK_BACK.0 as u32 => {
                self.query.pop();
                self.selected = 0;
                self.rebuild_results();
                let _ = InvalidateRect(self.hwnd, None, BOOL(1));
            }
            key if ctrl_down && is_app_mode_toggle_key(key) => self.toggle_app_mode(),
            key if ctrl_down
                && self.mode == OverlayMode::WindowsAndTabs
                && key == VK_D.0 as u32 =>
            {
                self.toggle_all_desktops()
            }
            key if ctrl_down && key == VK_RIGHT.0 as u32 => self.increase_thumbnail_size(),
            key if ctrl_down && key == VK_LEFT.0 as u32 => self.decrease_thumbnail_size(),
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
            key if key == VK_LEFT.0 as u32 => return self.selected_move_to_next_monitor(),
            key if self.mode == OverlayMode::WindowsAndTabs && key == VK_RIGHT.0 as u32 => {
                return self.selected_activation(true);
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
                self.apps = enumerate_apps();
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

    unsafe fn increase_thumbnail_size(&mut self) {
        if self.thumbnail_size_index + 1 < THUMBNAIL_SIZES.len() {
            self.thumbnail_size_index += 1;
            let _ = InvalidateRect(self.hwnd, None, BOOL(1));
        }
    }

    unsafe fn decrease_thumbnail_size(&mut self) {
        if self.thumbnail_size_index > 0 {
            self.thumbnail_size_index -= 1;
            let _ = InvalidateRect(self.hwnd, None, BOOL(1));
        }
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
            (OverlayMode::Apps, _) => "No matching installed apps",
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
        }))
    }

    unsafe fn selected_move_to_next_monitor(&mut self) -> DeferredAction {
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

        DeferredAction::MoveToNextMonitor(Box::new(MoveWindowRequest {
            hwnd,
            overlay_hwnd: self.hwnd,
            original_rect,
        }))
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
        self.draw_search(hdc, rect);
        self.draw_results(hdc, rect);
        if self.help_hovered {
            self.draw_help_tooltip(hdc, rect);
        }
        self.update_thumbnails();

        let _ = EndPaint(self.hwnd, &ps);
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
            (OverlayMode::Apps, _, true) => "Type to search installed apps".to_string(),
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
            };

            if result.kind == SearchResultKind::App {
                let old_app = SelectObject(hdc, screen_font);
                SetTextColor(hdc, rgb(188, 194, 200));
                draw_text(
                    hdc,
                    "APP",
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
        if help_hovered != self.help_hovered || close_hovered != self.close_hovered {
            self.help_hovered = help_hovered;
            self.close_hovered = close_hovered;
            let _ = InvalidateRect(self.hwnd, None, BOOL(1));
        }
    }

    unsafe fn on_left_button_up(&mut self, x: i32, y: i32) {
        if self.visible && point_in_rect(self.close_rect, x, y) {
            self.hide();
        }
    }

    unsafe fn on_mouse_leave(&mut self) {
        self.mouse_tracking = false;
        if self.help_hovered || self.close_hovered {
            self.help_hovered = false;
            self.close_hovered = false;
            let _ = InvalidateRect(self.hwnd, None, BOOL(1));
        }
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        unsafe {
            self.remove_tray_icon();
            self.unregister_thumbnails();
            if !self.hwnd.0.is_null() {
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
    MoveToNextMonitor(Box<MoveWindowRequest>),
}

struct ActivationRequest {
    result: SearchResult,
    windows: Vec<WindowEntry>,
    bridge: ExtensionBridge,
    overlay_hwnd: HWND,
    restore_overlay_focus: bool,
}

struct MoveWindowRequest {
    hwnd: isize,
    overlay_hwnd: HWND,
    original_rect: RECT,
}

impl DeferredAction {
    unsafe fn run(self) -> bool {
        match self {
            Self::None => false,
            Self::Activate(request) => {
                run_activation(*request);
                false
            }
            Self::MoveToNextMonitor(request) => run_move_to_next_monitor(*request),
        }
    }
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
            if action.run() {
                with_state_mut(state_ptr, "refresh after deferred action", (), |state| {
                    state.refresh()
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
            if !set_run_at_startup(!enabled) {
                log_runtime_issue("Failed to update the Windows startup registry value.");
            }
        }
        TrayMenuCommand::Exit => {
            let _ = DestroyWindow(hwnd);
        }
    }
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

unsafe fn run_activation(request: ActivationRequest) {
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
    }

    if request.restore_overlay_focus {
        restore_overlay_focus(request.overlay_hwnd);
    }
}

unsafe fn run_move_to_next_monitor(request: MoveWindowRequest) -> bool {
    let hwnd = hwnd_from_isize(request.hwnd);
    if !IsWindow(hwnd).as_bool() {
        return false;
    }

    move_window_to_overlay_desktop_if_needed(hwnd, request.overlay_hwnd);

    let moved = move_window_to_next_monitor(hwnd, request.original_rect);
    if moved {
        restore_overlay_focus(request.overlay_hwnd);
    }
    moved
}

fn selected_move_target_hwnd(result: &SearchResult, windows: &[WindowEntry]) -> Option<isize> {
    match &result.target {
        ActivationTarget::Window { hwnd } => Some(*hwnd),
        ActivationTarget::Tab { parent_hwnd, .. } => *parent_hwnd,
        ActivationTarget::App { .. } => running_window_for_app(&result.title, windows),
    }
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

fn result_thumbnail_hwnd(result: &SearchResult) -> Option<isize> {
    match &result.target {
        ActivationTarget::Window { hwnd } => Some(*hwnd),
        ActivationTarget::Tab { parent_hwnd, .. } => *parent_hwnd,
        ActivationTarget::App { .. } => None,
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
