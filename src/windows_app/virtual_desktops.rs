use mega_win_alt_tab::core::DesktopLocation;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};

pub(super) unsafe fn virtual_desktop_manager() -> Option<IVirtualDesktopManager> {
    CoCreateInstance::<_, IVirtualDesktopManager>(
        &VirtualDesktopManager,
        None,
        CLSCTX_INPROC_SERVER,
    )
    .ok()
}

pub(super) unsafe fn window_desktop_location(
    manager: Option<&IVirtualDesktopManager>,
    hwnd: HWND,
) -> DesktopLocation {
    let Some(manager) = manager else {
        return DesktopLocation::Unknown;
    };

    match manager.IsWindowOnCurrentVirtualDesktop(hwnd) {
        Ok(on_current) if on_current.as_bool() => DesktopLocation::Current,
        Ok(_) => DesktopLocation::Other,
        Err(_) => DesktopLocation::Unknown,
    }
}

pub(super) fn should_include_window_for_desktop(
    include_all_desktops: bool,
    desktop_location: DesktopLocation,
    cloaked: bool,
) -> bool {
    if !include_all_desktops && desktop_location == DesktopLocation::Other {
        return false;
    }

    !cloaked || (include_all_desktops && desktop_location == DesktopLocation::Other)
}

pub(super) unsafe fn move_window_to_overlay_desktop_if_needed(
    hwnd: HWND,
    overlay_hwnd: HWND,
) -> bool {
    let Some(manager) = virtual_desktop_manager() else {
        return false;
    };

    let Ok(on_current) = manager.IsWindowOnCurrentVirtualDesktop(hwnd) else {
        return false;
    };
    if on_current.as_bool() {
        return false;
    }

    let Ok(current_desktop_id) = manager.GetWindowDesktopId(overlay_hwnd) else {
        return false;
    };

    manager
        .MoveWindowToDesktop(hwnd, &current_desktop_id)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_filter_keeps_current_windows_in_normal_mode() {
        assert!(should_include_window_for_desktop(
            false,
            DesktopLocation::Current,
            false
        ));
        assert!(should_include_window_for_desktop(
            true,
            DesktopLocation::Current,
            false
        ));
    }

    #[test]
    fn desktop_filter_hides_other_desktop_windows_until_all_desktops_mode() {
        assert!(!should_include_window_for_desktop(
            false,
            DesktopLocation::Other,
            false
        ));
        assert!(should_include_window_for_desktop(
            true,
            DesktopLocation::Other,
            false
        ));
    }

    #[test]
    fn desktop_filter_keeps_cloaked_windows_only_when_they_are_off_desktop() {
        assert!(!should_include_window_for_desktop(
            true,
            DesktopLocation::Current,
            true
        ));
        assert!(!should_include_window_for_desktop(
            true,
            DesktopLocation::Unknown,
            true
        ));
        assert!(should_include_window_for_desktop(
            true,
            DesktopLocation::Other,
            true
        ));
    }
}
