//! The console's colours (M4 hosting spec §3.2): DESIGN.md's tokens, exact where the terminal has
//! 24-bit colour, the nearest of 256 elsewhere, and the terminal's own with `NO_COLOR`.
use ratatui::style::Color;

/// The colours a terminal shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Colours {
    /// 24-bit colour: the design's colours exactly.
    True,
    /// The xterm palette's 256: the nearest of them.
    Indexed,
    /// None (`NO_COLOR`): the terminal's own, with bold and reverse video where colour would tell.
    None,
}

impl Colours {
    /// From `COLORTERM` (`truecolor` or `24bit` means 24-bit colour), and whether `NO_COLOR` asks
    /// for none.
    pub fn detect(colorterm: Option<&str>, no_color: bool) -> Colours {
        if no_color {
            return Colours::None;
        }
        match colorterm {
            Some(v) if v.eq_ignore_ascii_case("truecolor") || v.eq_ignore_ascii_case("24bit") => {
                Colours::True
            }
            _ => Colours::Indexed,
        }
    }

    /// [`Colours::detect`] with this process's environment. `NO_COLOR` counts when it's set to
    /// anything but the empty string, as its convention says.
    pub fn from_env() -> Colours {
        let colorterm = std::env::var("COLORTERM").ok();
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        Colours::detect(colorterm.as_deref(), no_color)
    }
}

/// The design's twelve colours, as a terminal shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Which colours these are. With [`Colours::None`] every one is [`Color::Reset`].
    pub colours: Colours,
    /// The log's ground.
    pub ink_950: Color,
    /// The title bar and the prompt row.
    pub ink_900: Color,
    /// The status strip and the command palette.
    pub ink_850: Color,
    /// The palette's chosen row, and the hairline.
    pub ink_800: Color,
    /// The line being typed, answers, and WARN and ERROR messages.
    pub text: Color,
    /// The log's messages, the title and the facts.
    pub text_2: Color,
    /// INFO, and what the palette's commands do.
    pub muted: Color,
    /// Times, separators and hints.
    pub faint: Color,
    /// gold-300: names of people and of the server, and the prompt.
    pub gold: Color,
    pub ok: Color,
    pub warn: Color,
    pub danger: Color,
}

const INK_950: u32 = 0x0c0c0f;
const INK_900: u32 = 0x111115;
const INK_850: u32 = 0x16161b;
const INK_800: u32 = 0x1c1c22;
const TEXT: u32 = 0xf2efe9;
const TEXT_2: u32 = 0xc9c2b7;
const MUTED: u32 = 0xa8a196;
const FAINT: u32 = 0x8c867c;
const GOLD_300: u32 = 0xf2b365;
const OK: u32 = 0x7fd38f;
const WARN: u32 = 0xe8b04a;
const DANGER: u32 = 0xf26d7d;

/// A token's red, green and blue.
fn rgb(token: u32) -> (u8, u8, u8) {
    let [_, r, g, b] = token.to_be_bytes();
    (r, g, b)
}

impl Palette {
    pub fn of(colours: Colours) -> Palette {
        let colour = |token: u32| {
            let (r, g, b) = rgb(token);
            match colours {
                Colours::True => Color::Rgb(r, g, b),
                Colours::Indexed => Color::Indexed(nearest_256(r, g, b)),
                Colours::None => Color::Reset,
            }
        };
        let mut palette = Palette {
            colours,
            ink_950: colour(INK_950),
            ink_900: colour(INK_900),
            ink_850: colour(INK_850),
            ink_800: colour(INK_800),
            text: colour(TEXT),
            text_2: colour(TEXT_2),
            muted: colour(MUTED),
            faint: colour(FAINT),
            gold: colour(GOLD_300),
            ok: colour(OK),
            warn: colour(WARN),
            danger: colour(DANGER),
        };
        if colours == Colours::Indexed {
            // The inks are layers, from the log's ground up to the palette's chosen row. In 256
            // colours ink-850 and ink-800 are nearest to the same grey, which would hide the
            // chosen row: each ink takes at least the grey step above the one below it.
            let mut inks = [
                palette.ink_950,
                palette.ink_900,
                palette.ink_850,
                palette.ink_800,
            ];
            for i in 1..inks.len() {
                if let (Color::Indexed(below), Color::Indexed(ink)) = (inks[i - 1], inks[i])
                    && ink <= below
                {
                    inks[i] = Color::Indexed(below.saturating_add(1));
                }
            }
            [
                palette.ink_950,
                palette.ink_900,
                palette.ink_850,
                palette.ink_800,
            ] = inks;
        }
        palette
    }
}

/// The 6×6×6 cube's levels, for each of red, green and blue.
const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// The xterm-256 colour nearest `(r, g, b)` by distance in RGB: a colour of the 6×6×6 cube (16 to
/// 231) or of the grey ramp (232 to 255), the cube's on a tie. The first sixteen are left out:
/// terminals' themes change them.
pub fn nearest_256(r: u8, g: u8, b: u8) -> u8 {
    let distance = |(r2, g2, b2): (u8, u8, u8)| {
        let d = |a: u8, b: u8| u32::from(a.abs_diff(b)).pow(2);
        d(r, r2) + d(g, g2) + d(b, b2)
    };
    // The cube's nearest is each channel's nearest level.
    let level = |v: u8| {
        let mut best = 0;
        for (i, l) in LEVELS.iter().enumerate() {
            if v.abs_diff(*l) < v.abs_diff(LEVELS[best]) {
                best = i;
            }
        }
        best
    };
    let (ri, gi, bi) = (level(r), level(g), level(b));
    let cube = (LEVELS[ri], LEVELS[gi], LEVELS[bi]);
    // The grey ramp runs from 8 to 238 in steps of 10.
    let grey_of = |i: u8| {
        let v = 8 + 10 * i;
        (v, v, v)
    };
    let mut grey = 0;
    for i in 1..24 {
        if distance(grey_of(i)) < distance(grey_of(grey)) {
            grey = i;
        }
    }
    if distance(cube) <= distance(grey_of(grey)) {
        // At most 16 + 36 × 5 + 6 × 5 + 5 = 231.
        (16 + 36 * ri + 6 * gi + bi) as u8
    } else {
        232 + grey
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_follow_the_terminal() {
        assert_eq!(Colours::detect(Some("truecolor"), false), Colours::True);
        assert_eq!(Colours::detect(Some("24bit"), false), Colours::True);
        assert_eq!(Colours::detect(None, false), Colours::Indexed);
        assert_eq!(Colours::detect(Some("truecolor"), true), Colours::None);
    }

    #[test]
    fn with_24_bit_colour_the_palette_is_the_design_exactly() {
        let p = Palette::of(Colours::True);
        assert_eq!(p.colours, Colours::True);
        let rgb = |c: u32| Color::Rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
        assert_eq!(
            [
                p.ink_950, p.ink_900, p.ink_850, p.ink_800, p.text, p.text_2, p.muted, p.faint,
                p.gold, p.ok, p.warn, p.danger,
            ],
            [
                0x0c0c0f, 0x111115, 0x16161b, 0x1c1c22, 0xf2efe9, 0xc9c2b7, 0xa8a196, 0x8c867c,
                0xf2b365, 0x7fd38f, 0xe8b04a, 0xf26d7d,
            ]
            .map(rgb)
        );
    }

    #[test]
    fn the_nearest_of_256_comes_from_the_cube_or_the_grey_ramp() {
        assert_eq!(nearest_256(0, 0, 0), 16);
        assert_eq!(nearest_256(255, 255, 255), 231);
        assert_eq!(nearest_256(0x5f, 0x87, 0xaf), 67);
        assert_eq!(nearest_256(128, 128, 128), 244);
        assert_eq!(nearest_256(0x8c, 0x86, 0x7c), 102);
        assert_eq!(nearest_256(0xf2, 0xef, 0xe9), 255);
    }

    #[test]
    fn with_256_colours_the_palette_is_the_nearest_and_keeps_the_inks_apart() {
        let p = Palette::of(Colours::Indexed);
        // ink-850 and ink-800 are nearest to the same grey; the selected palette row must still
        // show, so ink-800 takes the next step up.
        assert_eq!(
            [p.ink_950, p.ink_900, p.ink_850, p.ink_800],
            [232, 233, 234, 235].map(Color::Indexed)
        );
        assert_eq!(
            [
                p.text, p.text_2, p.muted, p.faint, p.gold, p.ok, p.warn, p.danger
            ],
            [255, 250, 247, 102, 215, 114, 179, 204].map(Color::Indexed)
        );
    }

    #[test]
    fn without_colours_the_palette_is_the_terminals_own() {
        let p = Palette::of(Colours::None);
        assert_eq!(p.colours, Colours::None);
        assert!(
            [
                p.ink_950, p.ink_900, p.ink_850, p.ink_800, p.text, p.text_2, p.muted, p.faint,
                p.gold, p.ok, p.warn, p.danger,
            ]
            .iter()
            .all(|c| *c == Color::Reset)
        );
    }
}
