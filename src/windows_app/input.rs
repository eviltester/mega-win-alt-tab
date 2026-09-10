use windows::Win32::Foundation::{LPARAM, RECT};

pub(super) fn point_in_rect(rect: RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

pub(super) fn mouse_point(lparam: LPARAM) -> (i32, i32) {
    let raw = lparam.0 as u32;
    let x = (raw & 0xffff) as i16 as i32;
    let y = ((raw >> 16) & 0xffff) as i16 as i32;
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mouse_lparam(x: i16, y: i16) -> LPARAM {
        let raw = (x as u16 as u32) | ((y as u16 as u32) << 16);
        LPARAM(raw as isize)
    }

    #[test]
    fn mouse_helpers_decode_signed_coordinates_and_hit_test_edges() {
        assert_eq!(mouse_point(mouse_lparam(-4, 12)), (-4, 12));

        let rect = RECT {
            left: 10,
            top: 20,
            right: 30,
            bottom: 40,
        };
        assert!(point_in_rect(rect, 10, 20));
        assert!(point_in_rect(rect, 29, 39));
        assert!(!point_in_rect(rect, 30, 39));
        assert!(!point_in_rect(rect, 29, 40));
    }
}
