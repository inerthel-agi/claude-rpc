//! The six UI styles: a dark and a light palette each, and their font.
//! OLED is the dark palette on pure black, whatever the style.

use crate::{Colors, Theme};
use slint::Color;
use std::path::Path;

pub(crate) const DESIGNS: [&str; 6] = ["classic", "bento", "tui", "split", "mica", "orbit"];

/// Palette in the order of the `Colors` fields: bg, panel, card, input, line,
/// frame, text, muted, accent, on-accent, track, hover, press, ok, danger, warn.
type Palette = [u32; 16];

// Secondary text on the dark palettes stays at #b0b0b0 or brighter: the
// software renderer has no hinting, and darker grey turns blurry on black.
const CLASSIC: [Palette; 2] = [
    [
        0x1a1918, 0x1f1e1c, 0x232220, 0x161514, 0x35342f, 0x45443f, 0xf5f4ef, 0xb6b3a8, 0xe08364,
        0xffffff, 0x33322e, 0x2a2927, 0x33322e, 0x4cb782, 0xe5484d, 0xd9a441,
    ],
    [
        0xfaf9f5, 0xf3f1ea, 0xffffff, 0xfbfaf7, 0xe6e3d9, 0xcbc8be, 0x191919, 0x6b6860, 0xc96442,
        0xffffff, 0xe6e3d9, 0xece9e0, 0xe2dfd5, 0x2e8f5f, 0xc92a2f, 0x946409,
    ],
];
const BENTO: [Palette; 2] = [
    [
        0x1e1f22, 0x1e1f22, 0x2b2d31, 0x1e1f22, 0x3f4147, 0x4e5058, 0xf2f3f5, 0xb5bac1, 0x5865f2,
        0xffffff, 0x404249, 0x35373c, 0x404249, 0x23a55a, 0xf23f43, 0xf0b232,
    ],
    [
        0xf2f3f5, 0xe3e5e8, 0xffffff, 0xf2f3f5, 0xe1e2e4, 0xc4c9ce, 0x313338, 0x5c5e66, 0x5865f2,
        0xffffff, 0xdfe1e5, 0xe8e9eb, 0xdfe1e5, 0x1f8b4c, 0xda373c, 0xa86d00,
    ],
];
const TUI: [Palette; 2] = [
    [
        0x0c0c0c, 0x0c0c0c, 0x121212, 0x0c0c0c, 0x3d3d3d, 0x3d3d3d, 0xe8e8e8, 0xb0b0b0, 0xef8257,
        0x0c0c0c, 0x242424, 0x1c1c1c, 0x242424, 0x5fd787, 0xff6b6b, 0xe5c07b,
    ],
    [
        0xf4f1ea, 0xf4f1ea, 0xfbf9f4, 0xffffff, 0xb8b2a3, 0xb8b2a3, 0x1a1814, 0x6f6a5f, 0xb84d24,
        0xffffff, 0xe3ded2, 0xece8de, 0xe3ded2, 0x2b8a4e, 0xc0392b, 0x8a6100,
    ],
];
const MICA: [Palette; 2] = [
    [
        0x202020, 0x202020, 0x2b2b2b, 0x2d2d2d, 0x1d1d1d, 0x474747, 0xffffff, 0xb0b0b0, 0x60cdff,
        0x000000, 0x383838, 0x2d2d2d, 0x383838, 0x6ccb5f, 0xff99a4, 0xfce100,
    ],
    [
        0xf3f3f3, 0xf3f3f3, 0xfbfbfb, 0xffffff, 0xe5e5e5, 0xc4c4c4, 0x1b1b1b, 0x616161, 0x005fb8,
        0xffffff, 0xe0e0e0, 0xeaeaea, 0xe0e0e0, 0x0f7b0f, 0xc42b1c, 0x9d5d00,
    ],
];
const ORBIT: [Palette; 2] = [
    [
        0x050506, 0x0e0e10, 0x0f0f11, 0x1c1c1f, 0x2a2a2e, 0x3c3c41, 0xe8eaed, 0xb0b4b8, 0x8ab4f8,
        0x062e6f, 0x2a2a2d, 0x1d1d20, 0x2a2a2d, 0x81c995, 0xf28b82, 0xfdd663,
    ],
    [
        0xffffff, 0xf8f9fa, 0xffffff, 0xf1f3f4, 0xdadce0, 0xbdc1c6, 0x1f1f1f, 0x5f6368, 0x0b57d0,
        0xffffff, 0xe9eef6, 0xf1f3f4, 0xe9eef6, 0x137333, 0xb3261e, 0x8a5a00,
    ],
];

fn palettes(design: &str) -> &'static [Palette; 2] {
    match design {
        "bento" => &BENTO,
        "tui" => &TUI,
        "mica" => &MICA,
        "orbit" => &ORBIT,
        // Split Studio changes the layout, not the colours.
        _ => &CLASSIC,
    }
}

/// The dark palette on pure black: surfaces are set apart by their borders.
fn oled(dark: &Palette) -> Palette {
    let mut palette = *dark;
    palette[..4].copy_from_slice(&[0x000000, 0x000000, 0x0a0a0a, 0x000000]);
    palette[4] = 0x2a2a2a;
    palette[5] = 0x2a2a2a;
    palette[10..13].copy_from_slice(&[0x1f1f1f, 0x141414, 0x1e1e1e]);
    palette
}

fn colors(palette: &Palette) -> Colors {
    let c = |index: usize| {
        let rgb = palette[index];
        Color::from_rgb_u8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
    };
    Colors {
        bg: c(0),
        panel: c(1),
        card: c(2),
        input: c(3),
        line: c(4),
        frame: c(5),
        text: c(6),
        muted: c(7),
        accent: c(8),
        on_accent: c(9),
        track: c(10),
        hover: c(11),
        press: c(12),
        ok: c(13),
        danger: c(14),
        warn: c(15),
    }
}

fn has_font(file: &str) -> bool {
    std::env::var_os("SystemRoot")
        .is_some_and(|root| Path::new(&root).join("Fonts").join(file).is_file())
}

/// Cascadia Mono ships with Windows 11; older Windows has Consolas.
pub(crate) fn mono_font() -> &'static str {
    if has_font("CascadiaMono.ttf") {
        "Cascadia Mono"
    } else {
        "Consolas"
    }
}

fn font(design: &str) -> &'static str {
    match design {
        "tui" => mono_font(),
        // Windows 11's own font; Windows 10 lacks it and keeps Geist.
        "mica" | "orbit" if has_font("SegUIVar.ttf") => "Segoe UI Variable Text",
        _ => "Geist",
    }
}

/// A stored style name, or Classic.
pub(crate) fn known(design: &str) -> &'static str {
    DESIGNS
        .into_iter()
        .find(|known| *known == design)
        .unwrap_or("classic")
}

pub(crate) fn apply(theme: &Theme, design: &str) {
    let design = known(design);
    let [dark, light] = palettes(design);
    theme.set_dark_colors(colors(dark));
    theme.set_light_colors(colors(light));
    theme.set_oled_colors(colors(&oled(dark)));
    theme.set_font(font(design).into());
    theme.set_design(design.into());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey(rgb: u32) -> u32 {
        ((rgb >> 16 & 0xff) + (rgb >> 8 & 0xff) + (rgb & 0xff)) / 3
    }

    #[test]
    fn dark_palettes_keep_secondary_text_readable() {
        for design in DESIGNS {
            let [dark, light] = palettes(design);
            assert!(grey(dark[7]) >= 0xb0, "{design} muted {:06x}", dark[7]);
            assert!(grey(oled(dark)[7]) >= 0xb0, "{design} OLED muted");
            assert_eq!(oled(dark)[0], 0, "{design} OLED background");
            assert!(
                grey(light[7]) <= 0x70,
                "{design} light muted {:06x}",
                light[7]
            );
        }
        assert_eq!(known("orbit"), "orbit");
        assert_eq!(known("nope"), "classic");
    }
}
