//! The window icon, drawn in code: a black badge with the panel stripe and
//! a red play triangle.

use egui::IconData;

pub fn icon() -> IconData {
    const S: usize = 64;
    let mut rgba = vec![0u8; S * S * 4];
    let stripe = [
        [0xd8, 0x43, 0x2a],
        [0xec, 0x7b, 0x24],
        [0xef, 0xc2, 0x3b],
        [0xe7, 0xe0, 0xc9],
    ];
    for y in 0..S {
        for x in 0..S {
            let i = (y * S + x) * 4;
            let (dx, dy) = (
                (x as f32 - 31.5).abs() - 24.0,
                (y as f32 - 31.5).abs() - 24.0,
            );
            let outside = dx.max(0.0).hypot(dy.max(0.0)) - 7.5;
            if outside > 0.5 {
                continue;
            }
            let mut c = [0x17, 0x17, 0x17];
            if (50..54).contains(&y) && (8..56).contains(&x) {
                c = stripe[((x - 8) / 12).min(3)];
            }
            // A play triangle pointing right, from x 22 to 46, y 12 to 42.
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let half = (fx - 22.0) / 24.0 * 15.0;
            if (22.0..46.0).contains(&fx) && (fy - 27.0).abs() < 15.0 - half {
                c = [0xff, 0x5b, 0x36];
            }
            rgba[i..i + 3].copy_from_slice(&c);
            rgba[i + 3] = ((0.5 - outside).clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    IconData {
        rgba,
        width: S as u32,
        height: S as u32,
    }
}
