use std::collections::{HashMap, HashSet};
use std::mem::size_of;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFO,
    MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MonitorMoveDirection {
    Previous,
    Next,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MonitorEntry {
    handle: isize,
    number: u32,
    rect: RECT,
}

pub(super) unsafe fn enumerate_monitor_numbers() -> HashMap<isize, u32> {
    enumerate_monitor_entries()
        .into_iter()
        .map(|monitor| (monitor.handle, monitor.number))
        .collect()
}

unsafe fn enumerate_monitor_entries() -> Vec<MonitorEntry> {
    unsafe extern "system" fn callback(
        monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let monitors = &mut *(lparam.0 as *mut Vec<(isize, Option<u32>, RECT)>);
        if let Some((parsed_number, rect)) = monitor_details(monitor) {
            monitors.push((monitor.0 as isize, parsed_number, rect));
        }
        BOOL(1)
    }

    let mut monitors: Vec<(isize, Option<u32>, RECT)> = Vec::new();
    let _ = EnumDisplayMonitors(
        HDC(std::ptr::null_mut()),
        None,
        Some(callback),
        LPARAM(&mut monitors as *mut Vec<(isize, Option<u32>, RECT)> as isize),
    );

    let mut entries = Vec::new();
    let mut used_numbers = HashSet::new();
    let mut fallback_number = 1;
    for (monitor, parsed_number, rect) in monitors {
        let number = parsed_number
            .filter(|number| used_numbers.insert(*number))
            .unwrap_or_else(|| {
                while used_numbers.contains(&fallback_number) {
                    fallback_number += 1;
                }
                let number = fallback_number;
                used_numbers.insert(number);
                fallback_number += 1;
                number
            });
        entries.push(MonitorEntry {
            handle: monitor,
            number,
            rect,
        });
    }

    sort_monitor_entries_by_layout(&mut entries);
    entries
}

fn sort_monitor_entries_by_layout(entries: &mut [MonitorEntry]) {
    entries.sort_by(|a, b| {
        a.rect
            .top
            .cmp(&b.rect.top)
            .then_with(|| a.rect.left.cmp(&b.rect.left))
            .then_with(|| a.number.cmp(&b.number))
    });
}

unsafe fn monitor_details(monitor: HMONITOR) -> Option<(Option<u32>, RECT)> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    let info_ptr = &mut info as *mut MONITORINFOEXW as *mut MONITORINFO;
    if !GetMonitorInfoW(monitor, info_ptr).as_bool() {
        return None;
    }
    Some((
        parse_display_number(&utf16z_to_string(&info.szDevice)),
        info.monitorInfo.rcMonitor,
    ))
}

pub(super) unsafe fn window_screen_number(
    hwnd: HWND,
    monitor_numbers: &HashMap<isize, u32>,
) -> Option<u32> {
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    if monitor.is_invalid() {
        return None;
    }
    monitor_numbers.get(&(monitor.0 as isize)).copied()
}

pub(super) unsafe fn monitor_rect_for_window(hwnd: HWND) -> Option<RECT> {
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    if monitor.is_invalid() {
        return None;
    }

    monitor_details(monitor).map(|(_, rect)| rect)
}

pub(super) unsafe fn current_window_rect(hwnd: HWND) -> Option<RECT> {
    let mut window_rect = RECT::default();
    GetWindowRect(hwnd, &mut window_rect)
        .is_ok()
        .then_some(window_rect)
}

pub(super) unsafe fn move_window_to_monitor(
    hwnd: HWND,
    original_window_rect: RECT,
    direction: MonitorMoveDirection,
) -> bool {
    let monitors = enumerate_monitor_entries();
    if monitors.len() < 2 {
        return false;
    }

    let current_monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    if current_monitor.is_invalid() {
        return false;
    }

    let Some(current_index) = monitors
        .iter()
        .position(|monitor| monitor.handle == current_monitor.0 as isize)
    else {
        return false;
    };
    let current = monitors[current_index];
    let next_index = monitor_index_after_move(current_index, monitors.len(), direction);
    let next = monitors[next_index];

    let Some(window_rect) = current_window_rect(hwnd) else {
        return false;
    };

    let placement =
        next_monitor_window_placement(window_rect, original_window_rect, current.rect, next.rect);
    SetWindowPos(
        hwnd,
        None,
        placement.left,
        placement.top,
        placement.width,
        placement.height,
        SWP_NOZORDER | SWP_NOACTIVATE,
    )
    .is_ok()
}

fn monitor_index_after_move(
    current_index: usize,
    monitor_count: usize,
    direction: MonitorMoveDirection,
) -> usize {
    match direction {
        MonitorMoveDirection::Previous => (current_index + monitor_count - 1) % monitor_count,
        MonitorMoveDirection::Next => (current_index + 1) % monitor_count,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WindowPlacement {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

fn next_monitor_window_placement(
    window_rect: RECT,
    original_window_rect: RECT,
    current_monitor_rect: RECT,
    next_monitor_rect: RECT,
) -> WindowPlacement {
    let original_width = (original_window_rect.right - original_window_rect.left).max(1);
    let original_height = (original_window_rect.bottom - original_window_rect.top).max(1);
    let next_width = (next_monitor_rect.right - next_monitor_rect.left).max(1);
    let next_height = (next_monitor_rect.bottom - next_monitor_rect.top).max(1);
    let width = original_width.min(next_width);
    let height = original_height.min(next_height);

    let offset_x = window_rect.left - current_monitor_rect.left;
    let offset_y = window_rect.top - current_monitor_rect.top;
    let max_x = (next_width - width).max(0);
    let max_y = (next_height - height).max(0);

    WindowPlacement {
        left: next_monitor_rect.left + offset_x.clamp(0, max_x),
        top: next_monitor_rect.top + offset_y.clamp(0, max_y),
        width,
        height,
    }
}

fn utf16z_to_string(value: &[u16]) -> String {
    let len = value.iter().position(|ch| *ch == 0).unwrap_or(value.len());
    String::from_utf16_lossy(&value[..len])
}

fn parse_display_number(value: &str) -> Option<u32> {
    let digits = value
        .chars()
        .rev()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.chars().rev().collect::<String>().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(
        handle: isize,
        number: u32,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    ) -> MonitorEntry {
        MonitorEntry {
            handle,
            number,
            rect: RECT {
                left,
                top,
                right,
                bottom,
            },
        }
    }

    #[test]
    fn next_monitor_placement_preserves_offset_and_clamps_to_visible_area() {
        let current = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let next = RECT {
            left: 1920,
            top: 0,
            right: 3200,
            bottom: 720,
        };
        let window = RECT {
            left: 100,
            top: 900,
            right: 900,
            bottom: 1300,
        };

        assert_eq!(
            next_monitor_window_placement(window, window, current, next),
            WindowPlacement {
                left: 2020,
                top: 320,
                width: 800,
                height: 400,
            }
        );
    }

    #[test]
    fn next_monitor_placement_resizes_to_fit_smaller_screen() {
        let current = RECT {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1440,
        };
        let next = RECT {
            left: 2560,
            top: 0,
            right: 3840,
            bottom: 720,
        };
        let original = RECT {
            left: 100,
            top: 100,
            right: 2100,
            bottom: 1300,
        };

        assert_eq!(
            next_monitor_window_placement(original, original, current, next),
            WindowPlacement {
                left: 2560,
                top: 0,
                width: 1280,
                height: 720,
            }
        );
    }

    #[test]
    fn next_monitor_placement_restores_original_size_on_larger_screen() {
        let current = RECT {
            left: 0,
            top: 0,
            right: 1280,
            bottom: 720,
        };
        let next = RECT {
            left: 1280,
            top: 0,
            right: 3840,
            bottom: 1440,
        };
        let original = RECT {
            left: 100,
            top: 100,
            right: 2100,
            bottom: 1300,
        };
        let current_resized = RECT {
            left: 0,
            top: 0,
            right: 1280,
            bottom: 720,
        };

        assert_eq!(
            next_monitor_window_placement(current_resized, original, current, next),
            WindowPlacement {
                left: 1280,
                top: 0,
                width: 2000,
                height: 1200,
            }
        );
    }

    #[test]
    fn display_number_parser_uses_trailing_digits() {
        assert_eq!(parse_display_number(r"\\.\DISPLAY1"), Some(1));
        assert_eq!(parse_display_number(r"\\.\DISPLAY27"), Some(27));
        assert_eq!(parse_display_number("Display"), None);
    }

    #[test]
    fn monitor_direction_wraps_in_screen_order() {
        assert_eq!(
            monitor_index_after_move(0, 3, MonitorMoveDirection::Previous),
            2
        );
        assert_eq!(
            monitor_index_after_move(1, 3, MonitorMoveDirection::Previous),
            0
        );
        assert_eq!(
            monitor_index_after_move(2, 3, MonitorMoveDirection::Next),
            0
        );
        assert_eq!(
            monitor_index_after_move(0, 3, MonitorMoveDirection::Next),
            1
        );
    }

    #[test]
    fn monitor_layout_order_follows_visual_rows_not_display_numbers() {
        let mut monitors = vec![
            monitor(1, 1, 1920, 0, 3840, 1080),
            monitor(2, 2, 3840, 0, 5760, 1080),
            monitor(3, 3, 0, 0, 1920, 1080),
            monitor(4, 4, 1920, 1080, 3840, 2160),
        ];

        sort_monitor_entries_by_layout(&mut monitors);

        let numbers = monitors
            .iter()
            .map(|monitor| monitor.number)
            .collect::<Vec<_>>();
        assert_eq!(numbers, vec![3, 1, 2, 4]);

        let display_two_index = monitors
            .iter()
            .position(|monitor| monitor.number == 2)
            .unwrap();
        assert_eq!(
            monitors[monitor_index_after_move(
                display_two_index,
                monitors.len(),
                MonitorMoveDirection::Next
            )]
            .number,
            4
        );
        assert_eq!(
            monitors[monitor_index_after_move(
                display_two_index,
                monitors.len(),
                MonitorMoveDirection::Previous
            )]
            .number,
            1
        );
    }
}
