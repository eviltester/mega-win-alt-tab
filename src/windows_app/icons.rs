use windows::Win32::UI::WindowsAndMessaging::{CreateIcon, HICON};

pub(super) unsafe fn create_mega_icon() -> Option<HICON> {
    const ICON_SIZE: usize = 32;
    let xor_bits = mega_icon_xor_bits(ICON_SIZE);
    let mask_stride = ICON_SIZE.div_ceil(32) * 4;
    let and_mask = vec![0u8; mask_stride * ICON_SIZE];

    CreateIcon(
        None,
        ICON_SIZE as i32,
        ICON_SIZE as i32,
        1,
        32,
        and_mask.as_ptr(),
        xor_bits.as_ptr(),
    )
    .ok()
}

fn mega_icon_xor_bits(size: usize) -> Vec<u8> {
    let mut rgba = vec![0u8; size * size * 4];

    for y in 0..size {
        for x in 0..size {
            set_icon_pixel(&mut rgba, size, x, y, [19, 30, 39, 255]);
        }
    }

    fill_icon_rect(&mut rgba, size, 4, 4, 28, 28, [25, 42, 52, 255]);
    fill_icon_rect(&mut rgba, size, 7, 7, 14, 14, [70, 211, 220, 255]);
    fill_icon_rect(&mut rgba, size, 17, 7, 25, 14, [92, 143, 255, 255]);
    fill_icon_rect(&mut rgba, size, 7, 17, 14, 25, [73, 224, 144, 255]);
    fill_icon_rect(&mut rgba, size, 17, 17, 22, 22, [236, 245, 255, 255]);

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - 20.0;
            let dy = y as f32 - 20.0;
            let ring = (dx * dx + dy * dy - 35.0).abs() <= 9.5;
            let handle = distance_to_segment(x as f32, y as f32, 24.0, 24.0, 30.0, 30.0) <= 1.35;
            if ring || handle {
                set_icon_pixel(&mut rgba, size, x, y, [255, 206, 86, 255]);
            }
        }
    }

    rgba_to_bottom_up_bgra(&rgba, size)
}

fn fill_icon_rect(
    pixels: &mut [u8],
    size: usize,
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
    color: [u8; 4],
) {
    for y in top..bottom.min(size) {
        for x in left..right.min(size) {
            set_icon_pixel(pixels, size, x, y, color);
        }
    }
}

fn set_icon_pixel(pixels: &mut [u8], size: usize, x: usize, y: usize, color: [u8; 4]) {
    if x >= size || y >= size {
        return;
    }

    let index = (y * size + x) * 4;
    pixels[index..index + 4].copy_from_slice(&color);
}

fn rgba_to_bottom_up_bgra(rgba: &[u8], size: usize) -> Vec<u8> {
    let mut bgra = vec![0u8; rgba.len()];
    for y in 0..size {
        for x in 0..size {
            let src = (y * size + x) * 4;
            let dst = ((size - 1 - y) * size + x) * 4;
            bgra[dst] = rgba[src + 2];
            bgra[dst + 1] = rgba[src + 1];
            bgra[dst + 2] = rgba[src];
            bgra[dst + 3] = rgba[src + 3];
        }
    }
    bgra
}

fn distance_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let ab_x = bx - ax;
    let ab_y = by - ay;
    let ap_x = px - ax;
    let ap_y = py - ay;
    let ab_len_sq = ab_x * ab_x + ab_y * ab_y;
    if ab_len_sq == 0.0 {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }

    let t = ((ap_x * ab_x + ap_y * ab_y) / ab_len_sq).clamp(0.0, 1.0);
    let closest_x = ax + ab_x * t;
    let closest_y = ay + ab_y * t;
    ((px - closest_x).powi(2) + (py - closest_y).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains_bgra(bits: &[u8], color: [u8; 4]) -> bool {
        let mut index = 0;
        while index + 4 <= bits.len() {
            if bits[index..index + 4] == color {
                return true;
            }
            index += 4;
        }
        false
    }

    #[test]
    fn mega_icon_bits_have_expected_size_and_signature_colors() {
        let bits = mega_icon_xor_bits(32);

        assert_eq!(bits.len(), 32 * 32 * 4);
        assert!(contains_bgra(&bits, [39, 30, 19, 255]));
        assert!(contains_bgra(&bits, [220, 211, 70, 255]));
        assert!(contains_bgra(&bits, [86, 206, 255, 255]));
    }
}
