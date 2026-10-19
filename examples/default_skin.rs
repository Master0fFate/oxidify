//! Draws Oxidify's built-in Winamp skin: the one the skinned window
//! wears until the listener picks another, and the one any skin's missing
//! pieces are taken from. Winamp's own base skin cannot ship here, so this
//! is an original drawing in the colours of the Oxidify logo.
//!
//! `cargo run --example default_skin` regenerates
//! `assets/skins/oxidify.wsz`, a classic skin in every respect (BMPs in
//! a zip) that Winamp itself could wear. Add `--preview <png>` to also
//! compose the main window at 2x through the same sprite and layout tables
//! the app uses, as a check that the art lines up.

use std::path::PathBuf;

use oxidify::skin::layout::{self, Area};
use oxidify::skin::sprites::{self, Sheet, Sprite};
use oxidify::skin::{Skin, font, zip};

type Rgb = [u8; 3];
/// A box on a canvas: x, y, width, height.
type Box = (i64, i64, i64, i64);

const BLACK: Rgb = [0x05, 0x05, 0x08];
const CHASSIS: Rgb = [0x2c, 0x2c, 0x3f];
const CHASSIS_RIB: Rgb = [0x26, 0x26, 0x37];
const HIGHLIGHT: Rgb = [0x5c, 0x5c, 0x7c];
const SHADOW: Rgb = [0x12, 0x12, 0x1b];
const TITLE_BG: Rgb = [0x20, 0x20, 0x2f];
const FACE: Rgb = [0x3e, 0x3e, 0x57];
const FACE_PRESSED: Rgb = [0x2a, 0x2a, 0x3c];
const GLYPH: Rgb = [0xd2, 0xd2, 0xe2];
const LABEL: Rgb = [0xb0, 0xb0, 0xc6];
const LABEL_DIM: Rgb = [0x70, 0x70, 0x8c];
const TITLE: Rgb = [0xee, 0xee, 0xf6];
const LCD: Rgb = [0x00, 0x00, 0x00];
const LCD_GRID: Rgb = [0x16, 0x16, 0x26];
const LCD_TEXT: Rgb = [0x9c, 0x9c, 0xff];
const LCD_OFF: Rgb = [0x26, 0x26, 0x4c];
const LAMP_ON: Rgb = [0xff, 0x86, 0x3e];
const LAMP_OFF: Rgb = [0x4a, 0x26, 0x18];
const LAMP_LIGHT: Rgb = [0xff, 0xb8, 0x8a];
const LAMP_DARK: Rgb = [0x8a, 0x3c, 0x16];
const METAL: Rgb = [0x9a, 0x9a, 0xb4];
const METAL_LIGHT: Rgb = [0xd4, 0xd4, 0xe4];
const METAL_DARK: Rgb = [0x3a, 0x3a, 0x4e];
const GROOVE: Rgb = [0x14, 0x14, 0x20];
const OXIDE: [Rgb; 3] = [[0x7a, 0x7a, 0xff], [0xc0, 0x62, 0x9c], [0xff, 0x78, 0x3a]];

fn oxide(t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0) * 2.0;
    let (from, to, local) = if t < 1.0 {
        (OXIDE[0], OXIDE[1], t)
    } else {
        (OXIDE[1], OXIDE[2], t - 1.0)
    };
    std::array::from_fn(|channel| {
        let (from, to) = (f32::from(from[channel]), f32::from(to[channel]));
        (from + (to - from) * local).round() as u8
    })
}

/// The 5x6 bitmap font, drawn four pixels wide with a blank column after
/// (M, V, and W take the fifth). Rows are separated by `/`; `#` is a pixel.
const FONT: &[(char, &str)] = &[
    ('A', ".##./#..#/####/#..#/#..#"),
    ('B', "###./#..#/###./#..#/###."),
    ('C', ".###/#.../#.../#.../.###"),
    ('D', "###./#..#/#..#/#..#/###."),
    ('E', "####/#.../###./#.../####"),
    ('F', "####/#.../###./#.../#..."),
    ('G', ".###/#.../#.##/#..#/.###"),
    ('H', "#..#/#..#/####/#..#/#..#"),
    ('I', "###/.#./.#./.#./###"),
    ('J', "..##/...#/...#/#..#/.##."),
    ('K', "#..#/#.#./##../#.#./#..#"),
    ('L', "#.../#.../#.../#.../####"),
    ('M', "#...#/##.##/#.#.#/#...#/#...#"),
    ('N', "#..#/##.#/#.##/#..#/#..#"),
    ('O', ".##./#..#/#..#/#..#/.##."),
    ('P', "###./#..#/###./#.../#..."),
    ('Q', ".##./#..#/#..#/#.#./.#.#"),
    ('R', "###./#..#/###./#.#./#..#"),
    ('S', ".###/#.../.##./...#/###."),
    ('T', "###/.#./.#./.#./.#."),
    ('U', "#..#/#..#/#..#/#..#/.##."),
    ('V', "#...#/#...#/.#.#./.#.#./..#.."),
    ('W', "#...#/#...#/#.#.#/##.##/#...#"),
    ('X', "#..#/#..#/.##./#..#/#..#"),
    ('Y', "#..#/#..#/.##./.#../.#.."),
    ('Z', "####/...#/.##./#.../####"),
    ('"', ".#.#/.#.#"),
    ('@', ".###/#..#/#.##/#.../.###"),
    ('0', ".##./#..#/#..#/#..#/.##."),
    ('1', ".#../##../.#../.#../###."),
    ('2', "###./...#/.##./#.../####"),
    ('3', "###./...#/.##./...#/###."),
    ('4', "#..#/#..#/####/...#/...#"),
    ('5', "####/#.../###./...#/###."),
    ('6', ".##./#.../###./#..#/.##."),
    ('7', "####/...#/..#./.#../.#.."),
    ('8', ".##./#..#/.##./#..#/.##."),
    ('9', ".##./#..#/.###/...#/.##."),
    ('\u{2026}', "..../..../..../..../#.#.#"),
    ('.', "..../..../..../..../.#.."),
    (':', "..../.#../..../.#../...."),
    ('(', ".#./#../#../#../.#."),
    (')', "#../.#./.#./.#./#.."),
    ('-', "..../..../.###/..../...."),
    ('\'', ".#../.#../..../..../...."),
    ('!', ".#../.#../.#../..../.#.."),
    ('_', "..../..../..../..../..../####"),
    ('+', "..../.#../###./.#../...."),
    ('\\', "#.../.#../.#../..#./...#"),
    ('/', "...#/..#./.#../.#../#..."),
    ('[', "##./#../#../#../##."),
    (']', "##./.#./.#./.#./##."),
    ('^', ".#../#.#./..../..../...."),
    ('&', ".#../#.#./.#../#.#./.##."),
    ('%', "#..#/...#/..#./.#../#..#"),
    (',', "..../..../..../..../.#../#..."),
    ('=', "..../####/..../####/...."),
    ('$', ".###/##../.##./..##/###."),
    ('#', ".#.#/####/.#.#/####/.#.#"),
    ('\u{c5}', ".##./..../.##./#..#/####/#..#"),
    ('\u{d6}', "#..#/..../.##./#..#/#..#/.##."),
    ('\u{c4}', "#..#/..../.##./#..#/####/#..#"),
    ('?', "###./...#/.##./..../.#.."),
    ('*', "..../#.#./.#../#.#./...."),
];

fn art(character: char) -> &'static str {
    if character == ' ' {
        return "";
    }
    let character = character.to_ascii_uppercase();
    FONT.iter()
        .find(|(c, _)| *c == character)
        .or_else(|| FONT.iter().find(|(c, _)| *c == '?'))
        .map(|(_, rows)| *rows)
        .unwrap_or("")
}

const PREVIOUS: &str = "#....#/#...##/#..###/#.####/#..###/#...##/#....#";
const PLAY: &str = "##..../###.../####../#####./####../###.../##....";
const PAUSE: &str = "##.##/##.##/##.##/##.##/##.##/##.##/##.##";
const STOP: &str = "######/######/######/######/######/######";
const NEXT: &str = "#....#/##...#/###..#/####.#/###..#/##...#/#....#";
const EJECT: &str = "...#.../..###../.#####./#######/......./#######/#######";
const CLOSE: &str = "#...#/.#.#./..#../.#.#./#...#";
const MINIMIZE: &str = "...../...../...../...../#####";
const SHADE: &str = "#####/...../...../...../.....";
const UNSHADE: &str = "...../...../#####/...../.....";

struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<Rgb>,
}

impl Canvas {
    fn new(width: u32, height: u32, fill: Rgb) -> Self {
        Self {
            width,
            height,
            pixels: vec![fill; (width * height) as usize],
        }
    }

    fn set(&mut self, x: i64, y: i64, color: Rgb) {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return;
        }
        self.pixels[(y as u32 * self.width + x as u32) as usize] = color;
    }

    fn rect(&mut self, x: i64, y: i64, width: i64, height: i64, color: Rgb) {
        for py in y..y + height {
            for px in x..x + width {
                self.set(px, py, color);
            }
        }
    }

    /// A one pixel outline just inside the box.
    fn frame(&mut self, (x, y, width, height): Box, color: Rgb) {
        self.rect(x, y, width, 1, color);
        self.rect(x, y + height - 1, width, 1, color);
        self.rect(x, y, 1, height, color);
        self.rect(x + width - 1, y, 1, height, color);
    }

    /// A filled box lit from the top left.
    fn bevel(&mut self, (x, y, width, height): Box, fill: Rgb, light: Rgb, dark: Rgb) {
        self.rect(x, y, width, height, fill);
        self.rect(x, y, width, 1, light);
        self.rect(x, y, 1, height, light);
        self.rect(x, y + height - 1, width, 1, dark);
        self.rect(x + width - 1, y, 1, height, dark);
    }

    fn sunken(&mut self, area: Box, fill: Rgb) {
        self.bevel(area, fill, SHADOW, HIGHLIGHT);
    }

    fn ribbed(&mut self, (x, y, width, height): Box) {
        for row in y..y + height {
            let color = if row % 2 == 0 { CHASSIS } else { CHASSIS_RIB };
            self.rect(x, row, width, 1, color);
        }
    }

    /// Pixel art from rows of `#` and `.`, its top left at (x, y).
    fn glyph(&mut self, x: i64, y: i64, rows: &str, color: Rgb) {
        for (dy, row) in rows.split('/').enumerate() {
            for (dx, cell) in row.chars().enumerate() {
                if cell == '#' {
                    self.set(x + dx as i64, y + dy as i64, color);
                }
            }
        }
    }

    /// Pixel art centred in a box, nudged down and right by `shift`.
    fn glyph_centred(&mut self, (x, y, width, height): Box, rows: &str, color: Rgb, shift: i64) {
        let glyph_width = rows
            .split('/')
            .map(|row| row.len() as i64)
            .max()
            .unwrap_or(0);
        let glyph_height = rows.split('/').count() as i64;
        self.glyph(
            x + (width - glyph_width) / 2 + shift,
            y + (height - glyph_height) / 2 + shift,
            rows,
            color,
        );
    }

    fn embossed_glyph(&mut self, area: Box, rows: &str, color: Rgb) {
        self.glyph_centred(area, rows, BLACK, 1);
        self.glyph_centred(area, rows, color, 0);
    }

    /// A line of the bitmap font, five pixels per character.
    fn text(&mut self, x: i64, y: i64, text: &str, color: Rgb) {
        for (index, character) in text.chars().enumerate() {
            self.glyph(x + 5 * index as i64, y, art(character), color);
        }
    }

    fn text_centred(&mut self, (x, y, width, height): Box, text: &str, color: Rgb) {
        let text_width = 5 * text.chars().count() as i64 - 1;
        self.text(
            x + (width - text_width) / 2,
            y + (height - 5) / 2,
            text,
            color,
        );
    }

    fn embossed_text(&mut self, (x, y, width, height): Box, text: &str, color: Rgb) {
        self.text_centred((x + 1, y + 1, width, height), text, BLACK);
        self.text_centred((x, y, width, height), text, color);
    }

    fn wave_mark(&mut self, x: i64, cy: i64, height: i64) {
        const BARS: [i64; 6] = [24, 64, 100, 56, 84, 36];
        for (index, share) in BARS.into_iter().enumerate() {
            let bar = ((height * share + 50) / 100).max(1);
            let color = oxide(index as f32 / 5.0);
            self.rect(x + 2 * index as i64, cy - bar / 2, 1, bar, color);
        }
        for dy in [-height / 4, 1, height / 4] {
            self.set(x + 12, cy + dy, OXIDE[2]);
        }
    }

    /// The canvas as a 24-bit BMP, the format Winamp reads.
    fn bmp(&self) -> Vec<u8> {
        let bytes: Vec<u8> = self.pixels.iter().flatten().copied().collect();
        let image = image::RgbImage::from_raw(self.width, self.height, bytes)
            .expect("the canvas is the size it says");
        let mut out = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut out, image::ImageFormat::Bmp)
            .expect("a bitmap encodes");
        out.into_inner()
    }
}

fn ribs(c: &mut Canvas, x0: i64, x1: i64, bar_y: i64, bar_height: i64, color: impl Fn(i64) -> Rgb) {
    for y in (bar_y + 3..bar_y + bar_height - 3).step_by(2) {
        for x in x0..x1 {
            c.set(x, y, color(x));
        }
    }
}

fn title_ribs(
    c: &mut Canvas,
    (x0, x1): (i64, i64),
    (bar_x, bar_y, bar_height): (i64, i64, i64),
    active: bool,
) {
    ribs(c, x0, x1, bar_y, bar_height, |x| {
        if active {
            oxide((x - bar_x) as f32 / 274.0)
        } else {
            LABEL_DIM
        }
    });
}

fn button_face(canvas: &mut Canvas, area: Box, pressed: bool) {
    let (x, y, width, height) = area;
    canvas.frame(area, BLACK);
    let inner = (x + 1, y + 1, width - 2, height - 2);
    if pressed {
        canvas.bevel(inner, FACE_PRESSED, SHADOW, FACE);
    } else {
        canvas.bevel(inner, FACE, HIGHLIGHT, SHADOW);
    }
}

/// A raised button with pixel art on it.
fn button(canvas: &mut Canvas, area: Box, pressed: bool, rows: &str) {
    button_face(canvas, area, pressed);
    if pressed {
        canvas.glyph_centred(area, rows, LAMP_ON, 1);
    } else {
        canvas.embossed_glyph(area, rows, GLYPH);
    }
}

/// A raised button with a word on it.
fn label_button(canvas: &mut Canvas, area: Box, pressed: bool, label: &str) {
    button_face(canvas, area, pressed);
    let (x, y, width, height) = area;
    let shift = i64::from(pressed);
    canvas.text_centred((x + shift, y + shift, width, height), label, LABEL);
}

fn toggle_button(canvas: &mut Canvas, area: Box, pressed: bool, on: bool, label: &str) {
    button_face(canvas, area, pressed);
    let (x, y, width, height) = area;
    let shift = i64::from(pressed);
    let text_width = 5 * label.chars().count() as i64 - 1;
    let left = x + (width - text_width - 5) / 2 + shift;
    let top = y + (height - 5) / 2 + shift;
    canvas.rect(left, top + 1, 3, 3, if on { LAMP_ON } else { LAMP_OFF });
    canvas.text(left + 5, top, label, if on { TITLE } else { LABEL });
}

fn small_button(canvas: &mut Canvas, x: i64, y: i64, pressed: bool) {
    if pressed {
        canvas.bevel((x, y, 9, 9), FACE_PRESSED, SHADOW, FACE);
    } else {
        canvas.bevel((x, y, 9, 9), FACE, HIGHLIGHT, SHADOW);
    }
}

/// A sunken display area.
fn display(canvas: &mut Canvas, area: Box) {
    canvas.sunken(area, LCD);
}

/// A slider's grip: a raised block with two grooves.
fn thumb(canvas: &mut Canvas, area: Box, pressed: bool) {
    let (x, y, width, height) = area;
    let (fill, light, dark) = if pressed {
        (LAMP_ON, LAMP_LIGHT, LAMP_DARK)
    } else {
        (METAL, METAL_LIGHT, METAL_DARK)
    };
    canvas.frame(area, BLACK);
    canvas.bevel((x + 1, y + 1, width - 2, height - 2), fill, light, dark);
    if width >= 8 && height >= 7 {
        canvas.rect(x + width / 2 - 2, y + 3, 1, height - 6, dark);
        canvas.rect(x + width / 2 + 1, y + 3, 1, height - 6, dark);
    }
}

fn main_sheet() -> Canvas {
    let mut c = Canvas::new(layout::WINDOW_WIDTH, layout::WINDOW_HEIGHT, CHASSIS);
    c.ribbed((0, 0, 275, 116));
    c.frame((0, 0, 275, 116), BLACK);
    c.rect(1, 1, 273, 1, HIGHLIGHT);
    c.rect(1, 1, 1, 114, HIGHLIGHT);
    c.rect(1, 114, 273, 1, SHADOW);
    c.rect(273, 1, 1, 114, SHADOW);
    // The display: time and visualiser on the left, the song, its
    // format, and the channel lamps in their own wells on the right.
    display(&mut c, (9, 21, 95, 46));
    for y in (43..59).step_by(2) {
        for x in (24..100).step_by(2) {
            c.set(x, y, LCD_GRID);
        }
    }
    display(&mut c, (108, 24, 160, 12));
    display(&mut c, (109, 41, 19, 10));
    display(&mut c, (154, 41, 14, 10));
    display(&mut c, (210, 39, 60, 16));
    c.text(131, 43, "KBPS", LABEL);
    c.text(171, 43, "KHZ", LABEL);
    // The colon of the time display is part of the background.
    for y in [29, 34] {
        c.rect(72, y, 2, 2, oxide((y - 26) as f32 / 12.0));
    }
    let about = layout::ABOUT;
    c.wave_mark(
        i64::from(about.x),
        i64::from(about.y + about.height / 2),
        13,
    );
    c
}

fn title_bar(c: &mut Canvas, x: i64, y: i64, active: bool, shade: bool) {
    c.bevel((x, y, 275, 14), TITLE_BG, HIGHLIGHT, SHADOW);
    let color = if active { TITLE } else { LABEL_DIM };
    if shade {
        c.embossed_text((x + 20, y + 1, 35, 12), "OXIDIFY", color);
        // Shade mode's tiny transport lives in the bar itself.
        let glyphs: [(i64, i64, &str); 6] = [
            (169, 7, "#..#/#.##/####/#.##/#..#"),
            (176, 10, "#..../##.../###../##.../#...."),
            (186, 9, "#.#/#.#/#.#/#.#/#.#"),
            (195, 9, "###/###/###/###/###"),
            (204, 10, "#..#/##.#/####/##.#/#..#"),
            (215, 10, ".#./###/.../###/###"),
        ];
        for (gx, width, rows) in glyphs {
            c.embossed_glyph((x + gx, y + 2, width, 10), rows, GLYPH);
        }
        // The time sits in a sunken box, as it does on the display.
        display(c, (x + 124, y + 2, 36, 10));
        display(c, (x + 226, y + 4, 17, 7));
    } else {
        c.embossed_text((x, y + 1, 275, 12), "OXIDIFY", color);
        let width = 5 * 7 - 1;
        let start = x + (275 - width) / 2;
        title_ribs(c, (x + 18, start - 6), (x, y, 14), active);
        title_ribs(c, (start + width + 7, x + 241), (x, y, 14), active);
    }
}

fn titlebar_sheet() -> Canvas {
    let mut c = Canvas::new(344, 87, TITLE_BG);
    title_bar(&mut c, 27, 0, true, false);
    title_bar(&mut c, 27, 15, false, false);
    title_bar(&mut c, 27, 29, true, true);
    title_bar(&mut c, 27, 42, false, true);
    // The bars Winamp swapped in for its easter egg: the same here.
    title_bar(&mut c, 27, 57, true, false);
    title_bar(&mut c, 27, 72, false, false);

    for (x, y, pressed) in [(0, 0, false), (0, 9, true)] {
        small_button(&mut c, x, y, pressed);
        let shift = i64::from(pressed);
        for (index, height) in [3i64, 7, 5, 4].into_iter().enumerate() {
            let color = oxide(index as f32 / 3.0);
            c.rect(
                x + 1 + 2 * index as i64 + shift,
                y + 4 - height / 2 + shift,
                1,
                height,
                color,
            );
        }
    }
    let small: [(i64, i64, &str); 8] = [
        (9, 0, MINIMIZE),
        (9, 9, MINIMIZE),
        (0, 18, SHADE),
        (9, 18, SHADE),
        (18, 0, CLOSE),
        (18, 9, CLOSE),
        (0, 27, UNSHADE),
        (9, 27, UNSHADE),
    ];
    for (index, (x, y, rows)) in small.into_iter().enumerate() {
        let pressed = index % 2 == 1;
        small_button(&mut c, x, y, pressed);
        let color = if pressed { LAMP_ON } else { GLYPH };
        c.glyph_centred((x, y, 9, 9), rows, color, i64::from(pressed));
    }

    // Shade mode's seek bar and its thumb, at rest, left, and right.
    display(&mut c, (0, 36, 17, 7));
    c.rect(17, 36, 3, 7, METAL);
    c.rect(20, 36, 3, 7, LAMP_ON);
    c.rect(23, 36, 3, 7, METAL);

    // The clutter bar: O A I D V down the left of the display.
    let letters: [(char, i64); 5] = [('O', 3), ('A', 11), ('I', 18), ('D', 25), ('V', 33)];
    for (x, color) in [(304, LABEL_DIM), (312, LCD_OFF)] {
        c.rect(x, 0, 8, 43, LCD);
        for (letter, y) in letters {
            c.glyph_centred((x, y, 8, 7), art(letter), color, 0);
        }
    }
    let selected: [(i64, i64, i64); 5] = [
        (304, 47, 8),
        (312, 55, 7),
        (320, 62, 7),
        (328, 69, 8),
        (336, 77, 7),
    ];
    for ((letter, _), (x, y, height)) in letters.into_iter().zip(selected) {
        c.rect(x, y, 8, height, LCD);
        c.glyph_centred((x, y, 8, height), art(letter), LCD_TEXT, 0);
    }
    c
}

fn cbuttons_sheet() -> Canvas {
    let mut c = Canvas::new(136, 36, CHASSIS);
    let buttons: [(i64, i64, i64, &str); 6] = [
        (0, 23, 18, PREVIOUS),
        (23, 23, 18, PLAY),
        (46, 23, 18, PAUSE),
        (69, 23, 18, STOP),
        (92, 22, 18, NEXT),
        (114, 22, 16, EJECT),
    ];
    for (x, width, height, rows) in buttons {
        button(&mut c, (x, 0, width, height), false, rows);
        button(&mut c, (x, height, width, height), true, rows);
    }
    c
}

fn shufrep_sheet() -> Canvas {
    let mut c = Canvas::new(92, 85, CHASSIS);
    let states = [(false, false), (true, false), (false, true), (true, true)];
    for (row, (pressed, on)) in states.into_iter().enumerate() {
        let y = 15 * row as i64;
        toggle_button(&mut c, (0, y, 28, 15), pressed, on, "REP");
        toggle_button(&mut c, (28, y, 47, 15), pressed, on, "SHUFFLE");
    }
    for (x, pressed) in [(0, false), (46, true)] {
        for (y, on) in [(61, false), (73, true)] {
            toggle_button(&mut c, (x, y, 23, 12), pressed, on, "EQ");
            toggle_button(&mut c, (x + 23, y, 23, 12), pressed, on, "PL");
        }
    }
    c
}

fn posbar_sheet() -> Canvas {
    let mut c = Canvas::new(307, 10, CHASSIS);
    c.sunken((0, 0, 248, 10), GROOVE);
    c.rect(3, 4, 242, 2, SHADOW);
    thumb(&mut c, (248, 0, 29, 10), false);
    thumb(&mut c, (278, 0, 29, 10), true);
    c
}

fn slider_track(c: &mut Canvas, x: i64, y: i64, width: i64) {
    c.sunken((x, y, width, 13), GROOVE);
    c.rect(x + 2, y + 5, width - 4, 3, SHADOW);
}

fn volume_sheet() -> Canvas {
    let mut c = Canvas::new(68, 433, CHASSIS);
    for frame in 0..i64::from(sprites::SLIDER_FRAMES) {
        let y = 15 * frame;
        slider_track(&mut c, 0, y, 68);
        for column in 0..64 * frame / 27 {
            c.rect(2 + column, y + 5, 1, 3, oxide(column as f32 / 63.0));
        }
    }
    thumb(&mut c, (15, 422, 14, 11), false);
    thumb(&mut c, (0, 422, 14, 11), true);
    c
}

fn balance_sheet() -> Canvas {
    let mut c = Canvas::new(47, 433, CHASSIS);
    for frame in 0..i64::from(sprites::SLIDER_FRAMES) {
        let y = 15 * frame;
        slider_track(&mut c, 9, y, 38);
        let centre = 9 + 19;
        let half = 16 * frame / 27;
        for offset in -half..=half {
            let color = oxide(offset.abs() as f32 / 16.0);
            c.rect(centre + offset, y + 5, 1, 3, color);
        }
        if frame == 0 {
            c.rect(centre, y + 4, 1, 5, LABEL_DIM);
        }
    }
    thumb(&mut c, (15, 422, 14, 11), false);
    thumb(&mut c, (0, 422, 14, 11), true);
    c
}

fn monoster_sheet() -> Canvas {
    let mut c = Canvas::new(56, 24, LCD);
    for (y, color) in [(0, LCD_TEXT), (12, LCD_OFF)] {
        c.text_centred((0, y, 29, 12), "STEREO", color);
        c.text_centred((29, y, 27, 12), "MONO", color);
    }
    c
}

fn playpaus_sheet() -> Canvas {
    let mut c = Canvas::new(42, 9, LCD);
    c.glyph(
        1,
        1,
        "#...../##..../###.../####../###.../##..../#.....",
        LCD_TEXT,
    );
    c.glyph(10, 1, "##.##/##.##/##.##/##.##/##.##/##.##/##.##", LCD_TEXT);
    c.rect(19, 1, 7, 7, LCD_TEXT);
    c.rect(39, 3, 3, 3, LAMP_ON);
    c
}

/// A seven-segment digit, nine by thirteen: bit 0 is the top bar, the
/// bits go clockwise from there, and bit 6 is the middle bar.
fn led_digit(c: &mut Canvas, x: i64, y: i64, segments: u8) {
    let bars: [Box; 7] = [
        (1, 0, 7, 2),  // top
        (7, 1, 2, 5),  // top right
        (7, 7, 2, 5),  // bottom right
        (1, 11, 7, 2), // bottom
        (0, 7, 2, 5),  // bottom left
        (0, 1, 2, 5),  // top left
        (1, 5, 7, 2),  // middle
    ];
    for (bit, (bx, by, width, height)) in bars.into_iter().enumerate() {
        if segments & (1 << bit) != 0 {
            for row in by..by + height {
                c.rect(x + bx, y + row, width, 1, oxide(row as f32 / 12.0));
            }
        }
    }
}

const DIGIT_SEGMENTS: [u8; 10] = [
    0b011_1111, // 0
    0b000_0110, // 1
    0b101_1011, // 2
    0b100_1111, // 3
    0b110_0110, // 4
    0b110_1101, // 5
    0b111_1101, // 6
    0b000_0111, // 7
    0b111_1111, // 8
    0b110_1111, // 9
];

fn numbers_sheet(extended: bool) -> Canvas {
    let mut c = Canvas::new(if extended { 108 } else { 99 }, 13, LCD);
    for (digit, segments) in DIGIT_SEGMENTS.into_iter().enumerate() {
        led_digit(&mut c, 9 * digit as i64, 0, segments);
    }
    if extended {
        led_digit(&mut c, 99, 0, 0b100_0000);
    }
    c
}

fn text_sheet() -> Canvas {
    let mut c = Canvas::new(155, 18, LCD);
    for (character, rows) in FONT {
        let (row, column) = font::cell(*character);
        c.glyph(i64::from(5 * column), i64::from(6 * row), rows, LCD_TEXT);
    }
    c
}

fn pledit_sheet() -> Canvas {
    let mut c = Canvas::new(280, 186, CHASSIS);
    // The title row, active above inactive: corner, title, tile, corner.
    for (y, active) in [(0, true), (21, false)] {
        c.bevel((0, y, 178, 20), TITLE_BG, HIGHLIGHT, SHADOW);
        let color = if active { TITLE } else { LABEL_DIM };
        c.embossed_text((26, y, 100, 20), "PLAYLIST", color);
        let width = 5 * 8 - 1;
        let start = 26 + (100 - width) / 2;
        let rib = if active { OXIDE[1] } else { LABEL_DIM };
        // Across the left corner, the tiles, and the title on both sides
        // of its word; the right corner keeps its buttons.
        for (x0, x1) in [
            (4, 25),
            (26, start - 6),
            (start + width + 7, 126),
            (127, 152),
        ] {
            ribs(&mut c, x0, x1, y, 20, |_| rib);
        }
        for x in [156, 166] {
            small_button(&mut c, x, y + 3, false);
        }
        c.glyph_centred((156, y + 3, 9, 9), SHADE, GLYPH, 0);
        c.glyph_centred((166, y + 3, 9, 9), CLOSE, GLYPH, 0);
    }
    // The sides: a plain left edge and a right edge with the scroll track.
    c.rect(0, 42, 1, 29, HIGHLIGHT);
    c.rect(50, 42, 1, 29, SHADOW);
    c.rect(36, 42, 8, 29, GROOVE);
    c.rect(35, 42, 1, 29, SHADOW);
    c.rect(44, 42, 1, 29, HIGHLIGHT);
    // Pressed title buttons and the scroll handles.
    for (x, rows) in [(52, CLOSE), (62, SHADE), (150, UNSHADE)] {
        small_button(&mut c, x, 42, true);
        c.glyph_centred((x, 42, 9, 9), rows, LAMP_ON, 1);
    }
    thumb(&mut c, (52, 53, 8, 18), false);
    thumb(&mut c, (61, 53, 8, 18), true);
    // Shade mode's bar: left end, tile, right end (inactive, then active).
    // A sunken strip runs along it for the song and its time; the
    // buttons sit on the plain bar at the right.
    for (x, y, width) in [(72, 42, 25), (72, 57, 25), (99, 57, 50), (99, 42, 50)] {
        c.rect(x, y, width, 14, TITLE_BG);
        c.rect(x, y, width, 1, HIGHLIGHT);
        c.rect(x, y + 13, width, 1, SHADOW);
    }
    c.rect(72, 42, 1, 14, HIGHLIGHT);
    c.rect(148, 42, 1, 29, SHADOW);
    for y in [42, 57] {
        // The left end's strip starts a little in; the tile's runs across;
        // the right end's stops short of the buttons.
        c.rect(75, y + 2, 22, 10, LCD);
        c.rect(75, y + 2, 22, 1, SHADOW);
        c.rect(75, y + 11, 22, 1, HIGHLIGHT);
        c.rect(75, y + 2, 1, 10, SHADOW);
        c.rect(72, y + 2, 25, 10, LCD);
        c.rect(72, y + 2, 25, 1, SHADOW);
        c.rect(72, y + 11, 25, 1, HIGHLIGHT);
        c.rect(99, y + 2, 27, 10, LCD);
        c.rect(99, y + 2, 27, 1, SHADOW);
        c.rect(99, y + 11, 27, 1, HIGHLIGHT);
        c.rect(125, y + 2, 1, 10, HIGHLIGHT);
    }
    // The tile at (72,57) overwrote the left end's start; redraw it.
    c.rect(72, 57 + 2, 3, 10, TITLE_BG);
    for (y, color) in [(42, GLYPH), (57, LABEL_DIM)] {
        c.glyph_centred((126, y + 3, 9, 9), UNSHADE, color, 0);
        c.glyph_centred((137, y + 3, 9, 9), CLOSE, color, 0);
    }
    // The bottom: both corners, the tile between them, the visualiser's
    // well, and a grip in the corner for resizing.
    c.bevel((0, 72, 276, 38), CHASSIS, HIGHLIGHT, SHADOW);
    // The left half's menus, painted and quiet: Oxidify has no files
    // to add, but a bottom without them looks unfinished.
    for (x, label) in [(14, "ADD"), (43, "REM"), (72, "SEL"), (101, "MISC")] {
        label_button(&mut c, (x, 80, 22, 18), false, label);
    }
    // The right half: the running time's box, the little transport, the
    // song's time, LIST OPTS, and the grip. The sheet's right half is a
    // pixel over from where it lands in the window.
    display(&mut c, (130, 78, 64, 12));
    let mini: [&str; 6] = [
        "#..#/#.##/####/#.##/#..#",
        "#..../##.../###../##.../#....",
        "#.#/#.#/#.#/#.#/#.#",
        "###/###/###/###/###",
        "#..#/##.#/####/##.#/#..#",
        ".#./###/.../###/###",
    ];
    for (index, rows) in mini.into_iter().enumerate() {
        c.embossed_glyph((129 + 10 * index as i64, 94, 10, 10), rows, GLYPH);
    }
    display(&mut c, (189, 93, 27, 12));
    label_button(&mut c, (232, 80, 22, 18), false, "");
    c.text(233, 83, "LIST", LABEL);
    c.text(233, 90, "OPTS", LABEL);
    for dy in 0..9 {
        for dx in 0..9 {
            if dx + dy >= 8 && (dx + dy) % 3 == 2 {
                c.set(265 + dx, 99 + dy, LABEL_DIM);
            }
        }
    }
    c.rect(179, 0, 25, 1, HIGHLIGHT);
    c.rect(179, 37, 25, 1, SHADOW);
    display(&mut c, (205, 0, 75, 38));
    c
}

/// The equalizer window and its parts: the sliders' 28 frames, the
/// thumbs, the buttons in their states, the graph and the colour the
/// curve takes at each of its rows, and the preamp's line.
fn eqmain_sheet() -> Canvas {
    let mut c = Canvas::new(275, 315, CHASSIS);
    c.ribbed((0, 0, 275, 116));
    c.frame((0, 0, 275, 116), BLACK);
    c.rect(1, 1, 273, 1, HIGHLIGHT);
    c.rect(1, 1, 1, 114, HIGHLIGHT);
    c.rect(1, 114, 273, 1, SHADOW);
    c.rect(273, 1, 1, 114, SHADOW);
    c.text(43, 37, "+12 DB", LABEL);
    c.text(48, 65, "+0 DB", LABEL);
    c.text(43, 96, "-12 DB", LABEL);
    c.text(9, 104, "PREAMP", LABEL);
    let labels = [
        "60", "170", "310", "600", "1K", "3K", "6K", "12K", "14K", "16K",
    ];
    for (band, label) in labels.iter().enumerate() {
        let centre = 78 + 18 * band as i64 + 7;
        let width = 5 * label.len() as i64 - 1;
        c.text(centre - width / 2, 104, label, LABEL);
    }
    for (y, active) in [(134, true), (149, false)] {
        c.bevel((0, y, 275, 14), TITLE_BG, HIGHLIGHT, SHADOW);
        let color = if active { TITLE } else { LABEL_DIM };
        c.embossed_text((0, y + 1, 275, 12), "EQUALIZER", color);
        let width = 5 * 9 - 1;
        let start = (275 - width) / 2;
        title_ribs(&mut c, (18, start - 6), (0, y, 14), active);
        title_ribs(&mut c, (start + width + 7, 241), (0, y, 14), active);
    }
    small_button(&mut c, 0, 116, false);
    c.glyph_centred((0, 116, 9, 9), CLOSE, GLYPH, 0);
    small_button(&mut c, 0, 125, true);
    c.glyph_centred((0, 125, 9, 9), CLOSE, LAMP_ON, 1);
    // ON and AUTO: off, on, off pressed, on pressed, left to right.
    for (x, pressed, on) in [
        (10, false, false),
        (69, false, true),
        (128, true, false),
        (187, true, true),
    ] {
        toggle_button(&mut c, (x, 119, 26, 12), pressed, on, "ON");
        toggle_button(&mut c, (x + 26, 119, 32, 12), pressed, on, "AUTO");
    }
    // The sliders: a groove with the level filled from the centre.
    for frame in 0..28i64 {
        let x = 13 + (frame % 14) * 15;
        let y = 164 + (frame / 14) * 65;
        c.sunken((x, y, 14, 63), GROOVE);
        c.rect(x + 5, y + 2, 4, 59, SHADOW);
        c.rect(x + 4, y + 31, 6, 1, LABEL_DIM);
        let level = ((27 - frame) * 62 + 13) / 27;
        for row in level.min(31)..=level.max(31) {
            if row != 31 || level == 31 {
                c.rect(x + 5, y + row, 4, 1, oxide(1.0 - row as f32 / 62.0));
            }
        }
    }
    thumb(&mut c, (0, 164, 11, 11), false);
    thumb(&mut c, (0, 176, 11, 11), true);
    label_button(&mut c, (224, 164, 44, 12), false, "PRESETS");
    label_button(&mut c, (224, 176, 44, 12), true, "PRESETS");
    display(&mut c, (0, 294, 113, 19));
    c.rect(1, 303, 111, 1, LCD_GRID);
    for row in 0..19i64 {
        c.set(115, 294 + row, oxide(1.0 - row as f32 / 18.0));
    }
    c.rect(0, 314, 113, 1, LABEL_DIM);
    c
}

const PLEDIT_TXT: &str = "[Text]\r\nNormal=#9C9CFF\r\nCurrent=#FF863E\r\nNormalBG=#000000\r\nSelectedBG=#2C2C58\r\nFont=Inter\r\n";

/// Background, grid, sixteen spectrum bands from the top down, the
/// oscilloscope's five shades, and the peak marks.
fn viscolor_txt() -> String {
    let line = |color: Rgb, note: &str| format!("{},{},{} // {note}", color[0], color[1], color[2]);
    let mut lines = vec![line(LCD, "background"), line(LCD_GRID, "grid")];
    for band in 0..16u8 {
        let color = oxide(1.0 - f32::from(band) / 15.0);
        lines.push(line(color, &format!("band {band}")));
    }
    for shade in [TITLE, LCD_TEXT, LCD_TEXT, OXIDE[1], OXIDE[1]] {
        lines.push(line(shade, "oscilloscope"));
    }
    lines.push(line(TITLE, "peaks"));
    lines.join("\r\n") + "\r\n"
}

/// Composes the main window from the sheets on disk, the way the app will.
fn preview(skin: &Skin) -> image::RgbaImage {
    let mut image = image::RgbaImage::new(layout::WINDOW_WIDTH, layout::WINDOW_HEIGHT);
    let mut blit = |sprite: Sprite, x: u32, y: u32| {
        let Some((bitmap, sprite)) = skin.sprite(sprite) else {
            return;
        };
        for dy in 0..sprite.height {
            for dx in 0..sprite.width {
                let Some(pixel) = bitmap.pixel(sprite.x + dx, sprite.y + dy) else {
                    continue;
                };
                if x + dx < image.width() && y + dy < image.height() {
                    image.put_pixel(x + dx, y + dy, image::Rgba(pixel));
                }
            }
        }
    };
    let placed: [(Sprite, Area); 12] = [
        (sprites::MAIN_BACKGROUND, Area::new(0, 0, 275, 116)),
        (sprites::TITLE_BAR_ACTIVE, layout::TITLE_BAR),
        (sprites::OPTIONS_BUTTON, layout::OPTIONS_BUTTON),
        (sprites::MINIMIZE_BUTTON, layout::MINIMIZE_BUTTON),
        (sprites::SHADE_BUTTON, layout::SHADE_BUTTON),
        (sprites::CLOSE_BUTTON, layout::CLOSE_BUTTON),
        (sprites::CLUTTER_BAR, layout::CLUTTER_BAR),
        (sprites::STATUS_PLAYING, layout::STATUS),
        (sprites::WORK_INDICATOR_OFF, layout::WORK_INDICATOR),
        (sprites::NUMS_EX_BLANK, layout::MINUS_EX),
        (sprites::MONO_OFF, layout::MONO),
        (sprites::STEREO_ON, layout::STEREO),
    ];
    for (sprite, area) in placed {
        blit(sprite, area.x, area.y);
    }
    for (digit, area) in [0, 3, 4, 7].into_iter().zip(layout::TIME_DIGITS) {
        blit(sprites::digit_ex(digit), area.x, area.y);
    }
    let mut text = |line: &str, area: Area| {
        for (index, character) in line.chars().enumerate() {
            let x = area.x + 5 * index as u32;
            if x + 5 <= area.x + area.width {
                blit(font::glyph(character), x, area.y);
            }
        }
    };
    text("OXIDIFY - A WINAMP SKIN *** ", layout::MARQUEE);
    text("320", layout::KBPS);
    text("44", layout::KHZ);
    let volume = layout::VOLUME;
    blit(sprites::volume_frame(19), volume.x, volume.y);
    blit(
        sprites::VOLUME_THUMB,
        volume.x + layout::VOLUME_TRAVEL * 7 / 10,
        volume.y + 1,
    );
    let balance = layout::BALANCE;
    blit(sprites::balance_frame(0), balance.x, balance.y);
    blit(
        sprites::BALANCE_THUMB,
        balance.x + layout::BALANCE_TRAVEL / 2,
        balance.y + 1,
    );
    let position = layout::POSITION;
    blit(sprites::POSITION_TRACK, position.x, position.y);
    blit(
        sprites::POSITION_THUMB,
        position.x + layout::POSITION_TRAVEL * 3 / 10,
        position.y,
    );
    let controls: [(Sprite, Area); 10] = [
        (sprites::EQ_OFF, layout::EQ_BUTTON),
        (sprites::PLAYLIST_ON, layout::PLAYLIST_BUTTON),
        (sprites::PREVIOUS, layout::PREVIOUS),
        (sprites::PLAY_PRESSED, layout::PLAY),
        (sprites::PAUSE, layout::PAUSE),
        (sprites::STOP, layout::STOP),
        (sprites::NEXT, layout::NEXT),
        (sprites::EJECT, layout::EJECT),
        (sprites::SHUFFLE_ON, layout::SHUFFLE),
        (sprites::REPEAT_OFF, layout::REPEAT),
    ];
    for (sprite, area) in controls {
        blit(sprite, area.x, area.y);
    }
    image
}

fn main() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/skins/oxidify.wsz");
    let sheets: [(Sheet, Canvas); 14] = [
        (Sheet::Main, main_sheet()),
        (Sheet::CButtons, cbuttons_sheet()),
        (Sheet::TitleBar, titlebar_sheet()),
        (Sheet::ShufRep, shufrep_sheet()),
        (Sheet::PosBar, posbar_sheet()),
        (Sheet::Volume, volume_sheet()),
        (Sheet::Balance, balance_sheet()),
        (Sheet::MonoSter, monoster_sheet()),
        (Sheet::PlayPaus, playpaus_sheet()),
        (Sheet::Numbers, numbers_sheet(false)),
        (Sheet::NumsEx, numbers_sheet(true)),
        (Sheet::Text, text_sheet()),
        (Sheet::PlEdit, pledit_sheet()),
        (Sheet::EqMain, eqmain_sheet()),
    ];
    let mut files: Vec<(String, Vec<u8>)> = sheets
        .iter()
        .map(|(sheet, canvas)| (format!("{}.bmp", sheet.file_stem()), canvas.bmp()))
        .collect();
    files.push(("pledit.txt".to_string(), PLEDIT_TXT.as_bytes().to_vec()));
    files.push(("viscolor.txt".to_string(), viscolor_txt().into_bytes()));
    let entries: Vec<(&str, &[u8], bool)> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice(), true))
        .collect();
    let archive = zip::write(&entries);
    std::fs::write(&path, &archive).expect("the skin can be written");
    println!(
        "wrote {} files ({} bytes) to {}",
        entries.len(),
        archive.len(),
        path.display()
    );

    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--preview") {
        let path = PathBuf::from(args.next().expect("--preview takes a path"));
        let skin = Skin::from_archive("Oxidify", &archive).expect("the skin just written reads");
        let window = preview(&skin);
        let scaled = image::imageops::resize(
            &window,
            window.width() * 2,
            window.height() * 2,
            image::imageops::FilterType::Nearest,
        );
        scaled.save(&path).expect("the preview can be written");
        println!("wrote the preview to {}", path.display());
    }
}
