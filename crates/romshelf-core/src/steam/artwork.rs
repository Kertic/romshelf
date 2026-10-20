//! Shortcut artwork: Steam's file name for each slot, and generated artwork in the style of the
//! Library tile (system colour, system label, initials) so every shortcut always has some.

use crate::model::ArtSlot;
use crate::{Error, Result};

/// Image types Steam reads from the grid folder. Writing one slot removes the others.
pub const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "ico"];

/// The grid file name (without extension) Steam uses for each slot of a non-Steam shortcut.
/// From the files on the Deck (EmuDeck's and Steam ROM Manager's shortcuts); Confirm against
/// artwork set through Steam's own interface.
pub fn stem(slot: ArtSlot, app_id: u32) -> String {
    match slot {
        ArtSlot::Capsule => format!("{app_id}p"),
        ArtSlot::WideCapsule => format!("{app_id}"),
        ArtSlot::Hero => format!("{app_id}_hero"),
        ArtSlot::Logo => format!("{app_id}_logo"),
        ArtSlot::Icon => format!("{app_id}_icon"),
    }
}

pub const SLOTS: [ArtSlot; 5] = [
    ArtSlot::Capsule,
    ArtSlot::WideCapsule,
    ArtSlot::Hero,
    ArtSlot::Logo,
    ArtSlot::Icon,
];

/// Steam's sizes. Logo has no generated version: it's a transparent title, and Steam shows the
/// game's name when there is none.
pub fn size(slot: ArtSlot) -> Option<(u32, u32)> {
    match slot {
        ArtSlot::Capsule => Some((600, 900)),
        ArtSlot::WideCapsule => Some((920, 430)),
        ArtSlot::Hero => Some((1920, 620)),
        ArtSlot::Icon => Some((256, 256)),
        ArtSlot::Logo => None,
    }
}

/// "TW" for "Tidewalker", "EC" for "Ember Circuit": the first letters of the first two words.
pub fn initials(name: &str) -> String {
    name.split(|c: char| c.is_whitespace() || matches!(c, '&' | ',' | '-' | ':' | '('))
        .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

/// Parses `#RRGGBB`.
fn colour(hex: &str) -> [u8; 3] {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x2B3F6B);
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

/// 5 × 7 pixel glyphs, one byte per row (low five bits, leftmost pixel highest).
fn glyph(c: char) -> [u8; 7] {
    match c {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 2, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 17, 25, 21, 19, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [31, 2, 4, 2, 1, 17, 14],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 30, 1, 1, 17, 14],
        '6' => [6, 8, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 2, 12],
        _ => [0; 7],
    }
}

struct Canvas {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

impl Canvas {
    fn new(w: u32, h: u32, fill: [u8; 3]) -> Canvas {
        let mut px = Vec::with_capacity((w * h * 3) as usize);
        for _ in 0..w * h {
            px.extend_from_slice(&fill);
        }
        Canvas { w, h, px }
    }

    fn rect(&mut self, x: u32, y: u32, w: u32, h: u32, c: [u8; 3]) {
        for yy in y..(y + h).min(self.h) {
            for xx in x..(x + w).min(self.w) {
                let i = ((yy * self.w + xx) * 3) as usize;
                self.px[i..i + 3].copy_from_slice(&c);
            }
        }
    }

    /// Draws `text` with each font pixel `scale` pixels square, from (x, y).
    fn text(&mut self, text: &str, x: u32, y: u32, scale: u32, c: [u8; 3]) {
        for (n, ch) in text.chars().flat_map(char::to_uppercase).enumerate() {
            let left = x + n as u32 * 6 * scale;
            for (row, bits) in glyph(ch).iter().enumerate() {
                for col in 0..5 {
                    if bits & (1 << (4 - col)) != 0 {
                        self.rect(left + col * scale, y + row as u32 * scale, scale, scale, c);
                    }
                }
            }
        }
    }

    fn png(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.w, self.h);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::Best);
            let mut w = enc
                .write_header()
                .map_err(|e| Error::External(format!("couldn't make artwork: {e}")))?;
            w.write_image_data(&self.px)
                .map_err(|e| Error::External(format!("couldn't make artwork: {e}")))?;
        }
        Ok(out)
    }
}

/// Generated artwork for a slot: the system colour, the system label top left, the game's
/// initials bottom left, as on the Library tile. `None` for Logo.
pub fn fallback(
    slot: ArtSlot,
    name: &str,
    system_label: &str,
    system_colour: &str,
) -> Result<Option<Vec<u8>>> {
    let Some((w, h)) = size(slot) else {
        return Ok(None);
    };
    let bg = colour(system_colour);
    let fg = [255, 255, 255];
    let mut c = Canvas::new(w, h, bg);
    let unit = (w.min(h) / 40).max(2);
    let margin = unit * 3;
    if slot != ArtSlot::Icon {
        c.text(system_label, margin, margin, unit, fg);
    }
    let big = if slot == ArtSlot::Icon {
        w / 14
    } else {
        unit * 4
    };
    let init = initials(name);
    let text_h = 7 * big;
    let y = if slot == ArtSlot::Icon {
        (h - text_h) / 2
    } else {
        h - margin - text_h
    };
    let x = if slot == ArtSlot::Icon {
        let text_w = (init.chars().count() as u32 * 6).saturating_sub(1) * big;
        w.saturating_sub(text_w) / 2
    } else {
        margin
    };
    c.text(&init, x, y, big, fg);
    c.png().map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_read_like_the_tile() {
        assert_eq!(initials("Tidewalker"), "T");
        assert_eq!(initials("Ember Circuit"), "EC");
        assert_eq!(initials("Moss & Lantern"), "ML");
        assert_eq!(initials("The Legend of Zelda"), "TL");
        assert_eq!(initials("Kirby - Nightmare in Dream Land"), "KN");
    }

    #[test]
    fn names_follow_steams_grid_folder() {
        assert_eq!(stem(ArtSlot::Capsule, 42), "42p");
        assert_eq!(stem(ArtSlot::WideCapsule, 42), "42");
        assert_eq!(stem(ArtSlot::Hero, 42), "42_hero");
        assert_eq!(stem(ArtSlot::Logo, 42), "42_logo");
        assert_eq!(stem(ArtSlot::Icon, 42), "42_icon");
    }
}
