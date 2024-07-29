#[derive(Debug, Clone, Copy)]
pub struct VdgModeDetails {
    pub cell_x: usize,     // width (in pixels) of each cell on the display
    pub cell_y: usize,     // height (in pixels) of each cell on the display
    pub color_bits: usize, // bits used in vram for each cell's color data (1 ==> luminance only)
}
// todo: consider making VdgMode into a struct (including VdgModeDetails *and* CSS)
// and turning the VdgMode enum into VdgModeType or some such
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VdgMode {
    // Note: Coco is hardwired such that Alpha modes are not supported.
    // Alphanumeric characters are displayed using SG4 mode instead.
    SG4,
    SG6,
    SG8,
    SG12,
    SG24,
    CG1,
    RG1,
    CG2,
    RG2,
    CG3,
    RG3,
    CG6,
    RG6,
}
impl VdgMode {
    pub fn get_details(&self) -> VdgModeDetails {
        match self {
            SG4 => VdgModeDetails {
                cell_x: 4,
                cell_y: 6,
                color_bits: 3,
            },
            SG6 => VdgModeDetails {
                cell_x: 4,
                cell_y: 4,
                color_bits: 2,
            },
            SG8 => VdgModeDetails {
                cell_x: 4,
                cell_y: 3,
                color_bits: 3,
            },
            SG12 => VdgModeDetails {
                cell_x: 4,
                cell_y: 2,
                color_bits: 3,
            },
            SG24 => VdgModeDetails {
                cell_x: 4,
                cell_y: 1,
                color_bits: 3,
            },
            CG1 => VdgModeDetails {
                cell_x: 4,
                cell_y: 3,
                color_bits: 2,
            },
            RG1 => VdgModeDetails {
                cell_x: 2,
                cell_y: 3,
                color_bits: 1,
            },
            CG2 => VdgModeDetails {
                cell_x: 2,
                cell_y: 3,
                color_bits: 2,
            },
            RG2 => VdgModeDetails {
                cell_x: 2,
                cell_y: 2,
                color_bits: 1,
            },
            CG3 => VdgModeDetails {
                cell_x: 2,
                cell_y: 2,
                color_bits: 2,
            },
            RG3 => VdgModeDetails {
                cell_x: 2,
                cell_y: 1,
                color_bits: 1,
            },
            CG6 => VdgModeDetails {
                cell_x: 2,
                cell_y: 1,
                color_bits: 2,
            },
            RG6 => VdgModeDetails {
                cell_x: 1,
                cell_y: 1,
                color_bits: 1,
            },
        }
    }
    /// Given VDG mode bits from both PIA1 and SAM, return the corresponding VDG mode (or None)
    pub fn try_from_pia_and_sam(pia: u8, sam: u8) -> Option<Self> {
        let pia = pia & 0b11110; // ignore CSS bit here
        let gagm0 = pia & 0b10010;
        match sam {
            0 if gagm0 == 0 => Some(SG4),
            0 if gagm0 == 2 => Some(SG6),
            1 if pia == 0b10000 => Some(CG1),
            1 if pia == 0b10010 => Some(RG1),
            2 if gagm0 == 0 => Some(SG8),
            2 if pia == 0b10100 => Some(CG2),
            3 if pia == 0b10110 => Some(RG2),
            4 if gagm0 == 0 => Some(SG12),
            4 if pia == 0b11000 => Some(CG3),
            5 if pia == 0b11010 => Some(RG3),
            6 if gagm0 == 0 => Some(SG24),
            6 if pia == 0b11100 => Some(CG6),
            6 if pia == 0b11110 => Some(RG6),
            // ignoring DMA mode?
            _ => None,
        }
    }
}
use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use VdgMode::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Black = 0,
    Green = 1,
    Yellow = 2,
    Blue = 3,
    Red = 4,
    Buff = 5,
    Cyan = 6,
    Magenta = 7,
    Orange = 8,
}
use Color::*;
impl Color {
    pub fn to_rgb(self) -> u32 {
        match self {
            Black => 0,
            Green => 0x0020e000,
            Yellow => 0x00fff000,
            Blue => 0x000000ff,
            Red => 0x00f00000,
            Buff => 0x00e0e0e0,
            Cyan => 0x0000efff,
            Magenta => 0x00d000d0,
            Orange => 0x00f06000,
        }
    }
    // pub fn to_code(self) -> u8 { self as u8 }
    pub fn from_code(color_code: u8) -> Self {
        match color_code {
            1 => Green,
            2 => Yellow,
            3 => Blue,
            4 => Red,
            5 => Buff,
            6 => Cyan,
            7 => Magenta,
            8 => Orange,
            _ => Black,
        }
    }
    pub fn from_3bits(bits: u8) -> Self { Color::from_code(bits + 1) }
    pub fn from_2bits(bits: u8, css: bool) -> Self { Color::from_code(1 + (bits | if css { 4 } else { 0 })) }
}
// Setting refresh rate to roughly 30 Hz (emulating NTSC)
pub const SCREEN_REFRESH_PERIOD: Duration = Duration::from_micros(33333);
pub const SCREEN_DIM_X: usize = 256;
pub const SCREEN_DIM_Y: usize = 192;
pub const BLOCK_DIM_X: usize = 8;
pub const BLOCK_DIM_Y: usize = 12;
pub const BLOCK_COLS: usize = SCREEN_DIM_X / BLOCK_DIM_X;
pub const BLOCK_ROWS: usize = SCREEN_DIM_Y / BLOCK_DIM_Y;
pub const VRAM_SIZE: usize = (SCREEN_DIM_X * SCREEN_DIM_Y) / 8;
pub const ALWAYS_RENDER: bool = true;

pub struct Char {
    font_index: usize,
    inverted: bool,
}
impl Char {
    #[inline(always)]
    pub fn try_from_ascii(byte: u8) -> Option<Self> {
        let i = match byte {
            0..=0x1f => 0x20,
            0x20..=0x3f => byte,
            0x40..=0x7f => byte & 0x1f,
            _ => return None,
        };
        Some(Char {
            font_index: (i as usize) * BLOCK_DIM_Y,
            inverted: byte > 0x5f,
        })
    }
    #[inline(always)]
    pub fn try_from_raw(byte: u8) -> Option<Self> {
        let (i, inverted) = match byte {
            0..=0x3f => (byte as usize, false),
            0x40..=0x7f => ((byte - 0x40) as usize, true),
            _ => return None,
        };
        Some(Char {
            font_index: i * BLOCK_DIM_Y,
            inverted,
        })
    }
}
/// NOTE: If using VDG and its shared ram buffer at the same time then the lock order must be VDG and then ram.
#[derive(Debug)]
pub struct Vdg {
    mode: VdgMode,
    dirty: bool,
    ram: &'static [u8],
    vram_offset: usize,
    ascii: bool,
}
unsafe impl Send for Vdg {}

impl Vdg {
    pub fn with_ram(ram: Arc<RwLock<Vec<u8>>>, vram_offset: usize/*, hsync: Arc<(Mutex<bool>, Condvar)>*/) -> Self {
        let mut ram = ram.write().unwrap();
        let ram = unsafe { std::slice::from_raw_parts(ram.as_mut_ptr(), ram.len()) };
        Vdg {
            mode: VdgMode::SG4,
            dirty: true,
            ram,
            vram_offset,
            ascii: false,
        }
    }

    pub fn set_mode(&mut self, mode: VdgMode) {
        if self.mode != mode {
            info!("VDG VdgMode changed from {:?} to {:?}", self.mode, mode);
            self.dirty = true;
            self.mode = mode;
        }
    }
    #[allow(unused)]
    pub fn interpret_chars_as_ascii(&mut self, ascii: bool) { self.ascii = ascii; }
    pub fn set_vram_offset(&mut self, vram_offset: usize) {
        if (vram_offset + VRAM_SIZE) > self.ram.len() {
            panic!(
                "invalid vram_offset {:x} (ram.len={:x}, VRAM_SIZE={:x}",
                vram_offset,
                self.ram.len(),
                VRAM_SIZE
            )
        }
        if vram_offset != self.vram_offset {
            info!(
                "VDG vram_offset changed from {:4x} to {:4x}",
                self.vram_offset, vram_offset
            );
            self.vram_offset = vram_offset;
            self.dirty = true;
        }
    }
    #[allow(unused)]
    pub fn get_mode(&self) -> VdgMode { self.mode }

    #[allow(unused)]
    pub fn set_dirty(&mut self) { self.dirty = true }

    // Renders the contents of VRAM to the provided buffer where each pixel is defined by a u32 formatted as 0x00RRGGBB
    // Returns true if any changes were made to the buffer.
    pub fn render(&mut self, display: &mut [u32], css: bool) -> bool {
        if !self.dirty && !ALWAYS_RENDER {
            return false;
        }
        self.dirty = false;
        match self.mode {
            SG4 => {
                for i in 0..(BLOCK_COLS * BLOCK_ROWS) {
                    let index = (((i / BLOCK_COLS) * BLOCK_DIM_Y) * SCREEN_DIM_X) + ((i % BLOCK_COLS) * BLOCK_DIM_X);
                    self.draw_sg4_block(display, index, self.ram[i + self.vram_offset], css);
                }
            }
            SG6 => {
                for i in 0..(BLOCK_COLS * BLOCK_ROWS) {
                    let index = (((i / BLOCK_COLS) * BLOCK_DIM_Y) * SCREEN_DIM_X) + ((i % BLOCK_COLS) * BLOCK_DIM_X);
                    self.draw_sg_block(display, index, self.ram[i + self.vram_offset], css);
                }
            }

            SG8 | SG12 | SG24 => self.render_sg_extended(display),
            _ => self.render_graphics(display, css),
        }
        true
    }
    fn render_graphics(&self, display: &mut [u32], css: bool) {
        let md = self.mode.get_details();
        let cells_per_src_byte = 8 / md.color_bits;
        let cells_per_row = SCREEN_DIM_X / md.cell_x;
        let cells_per_col = SCREEN_DIM_Y / md.cell_y;
        let src_bytes_per_row = cells_per_row / cells_per_src_byte;
        let mut dst_index = 0usize;
        let (fg_color, bg_color) = (Color::Green, Color::Black);
        for src_row in 0..cells_per_col {
            for _ in 0..md.cell_y {
                // repeat for each row in each cell
                for src_col in 0..src_bytes_per_row {
                    let src_index = self.vram_offset + src_col + src_row * src_bytes_per_row;
                    let mut src_data = self.ram[src_index] as u16;
                    for _ in 0..cells_per_src_byte {
                        let color = match md.color_bits {
                            1 => {
                                src_data <<= 1;
                                if src_data & 0x0100 == 0 { bg_color } else { fg_color }
                            }
                            2 => {
                                src_data <<= 2;
                                Color::from_2bits(((src_data & 0x300) >> 8) as u8, css)
                            }
                            _ => unreachable!(),
                        };
                        // draw all pixels for this pixel row of this cell
                        for _ in 0..md.cell_x {
                            display[dst_index] = color.to_rgb();
                            dst_index += 1;
                        }
                    }
                }
            }
        }
    }
    fn render_sg_extended(&self, display: &mut [u32]) {
        let md = self.mode.get_details();
        assert!(md.cell_x == 4 && md.cell_y < 12);
        let mut fg_color;
        let mut bg_color;
        // draw the screen column by column
        for block_col in 0..BLOCK_COLS {
            for block_row in 0..BLOCK_ROWS {
                let cell_rows = BLOCK_DIM_Y / md.cell_y;
                for cell_row in 0..cell_rows {
                    // each block is cell_rows high
                    // each cell_row in a block is defined by a byte in vram
                    // determine the index into vram where the source byte is stored
                    let src_index = self.vram_offset + block_col + (block_row * cell_rows + cell_row) * BLOCK_COLS;
                    // get the data defining this cell row
                    let cell_data = self.ram[src_index];
                    // if the byte represents an alphanumeric character then get it now
                    let ch = Char::try_from_ascii(cell_data);
                    // draw each row of pixels within the current cell(s)
                    // pix_row is a pixel row *within the current cell* (as opposed to the block or the screen)
                    for pix_row in 0..md.cell_y {
                        // determine the bit pattern to use for the current pixel_row of this cell
                        let pattern = if let Some(ch) = &ch {
                            // this cell contains alphanumeric character data so use the internal font
                            // but grab the pattern from the corresponding pixel row of the character in the font map
                            (fg_color, bg_color) = if ch.inverted { (Black, Green) } else { (Green, Black) };
                            !FONT_MAP[ch.font_index + pix_row + (cell_row * md.cell_y)]
                        } else {
                            // this is a block pattern
                            let mut p: u8 = 0;
                            if cell_data & 1 == 1 {
                                p |= 0xf
                            };
                            if cell_data & 2 == 2 {
                                p |= 0xf0
                            };
                            (fg_color, bg_color) = (Color::from_3bits((cell_data & 0x70) >> 4), Black);
                            p
                        };
                        // determine the index in the display where we're going to write these pixels
                        let dst_index = SCREEN_DIM_X * (block_row * BLOCK_DIM_Y + cell_row * md.cell_y + pix_row)
                            + block_col * BLOCK_DIM_X;
                        Vdg::draw_8_pixels(display, dst_index, pattern, fg_color, bg_color);
                    }
                }
            }
        }
    }

    fn draw_sg4_block(&self, display: &mut [u32], index: usize, glyph: u8, css: bool) {
        if glyph < 0x80 {
            // the glyph is an ascii character
            Vdg::draw_char_block(display, index, glyph, Color::Green, Color::Black, self.ascii);
        } else {
            // the glyph is an SG4 or SG6 block
            self.draw_sg_block(display, index, glyph, css);
        }
    }
    #[inline(always)]
    fn draw_char_block(display: &mut [u32], index: usize, glyph: u8, fg_color: Color, bg_color: Color, ascii: bool) {
        let ch = if ascii {
            Char::try_from_ascii(glyph)
        } else {
            Char::try_from_raw(glyph)
        };
        if let Some(ch) = ch {
            let (fg_color, bg_color) = if !ch.inverted {
                (fg_color, bg_color)
            } else {
                (bg_color, fg_color)
            };
            let mut font_index = ch.font_index;
            let mut font_line = 0;
            let mut dst_index = index;
            while font_line < BLOCK_DIM_Y {
                // for each line in the character's bitmap...
                Vdg::draw_8_pixels(display, dst_index, FONT_MAP[font_index], fg_color, bg_color);
                // update buffer and font indices
                dst_index += SCREEN_DIM_X;
                font_line += 1;
                font_index += 1;
            }
        }
    }
    #[inline(always)]
    fn draw_sg_block(&self, display: &mut [u32], index: usize, glyph: u8, css: bool) {
        let md = self.mode.get_details();
        let fg_color = if md.color_bits == 3 {
            Color::from_3bits((glyph & 0x70) >> 4)
        } else {
            Color::from_2bits((glyph & 0xc0) >> 6, css)
        };
        let row_pattern = |lum: u8| -> u8 {
            match lum {
                0 => 0,
                1 => 0x0f,
                2 => 0xf0,
                3 => 0xff,
                _ => unreachable!(),
            }
        };
        let cell_rows = BLOCK_DIM_Y / md.cell_y;
        let mut lum_mask = 0x3 << (2 * (cell_rows - 1));
        let mut dst_index = index;
        for cell_row in 0..cell_rows {
            let pattern = row_pattern((glyph & lum_mask) >> (2 * (cell_rows - cell_row - 1)));
            lum_mask >>= 2;
            for _ in 0..md.cell_y {
                Vdg::draw_8_pixels(display, dst_index, pattern, fg_color, Color::Black);
                dst_index += SCREEN_DIM_X;
            }
        }
    }
    #[inline(always)]
    fn draw_8_pixels(display: &mut [u32], index: usize, bits: u8, fg_color: Color, bg_color: Color) {
        let mut bit = 0x80u8;
        for i in 0..8 {
            if bits & bit != 0 {
                // the pixel is set (gets foreground color)
                display[index + i] = fg_color.to_rgb();
            } else {
                // the pixel is not set (gets background color)
                display[index + i] = bg_color.to_rgb();
            }
            bit >>= 1;
        }
    }
}
const FONT_MAP: &[u8] = &[
    0x00, 0x00, 0x00, 0x1C, 0x22, 0x2A, 0x2A, 0x2C, 0x20, 0x1E, 0x00, 0x00, // @
    0x00, 0x00, 0x00, 0x08, 0x14, 0x22, 0x22, 0x3E, 0x22, 0x22, 0x00, 0x00, // A
    0x00, 0x00, 0x00, 0x3C, 0x22, 0x22, 0x3C, 0x22, 0x22, 0x3C, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x1C, 0x22, 0x20, 0x20, 0x20, 0x22, 0x1C, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x3C, 0x22, 0x22, 0x22, 0x22, 0x22, 0x3C, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x3E, 0x20, 0x20, 0x3C, 0x20, 0x20, 0x3E, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x3E, 0x20, 0x20, 0x3C, 0x20, 0x20, 0x20, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x1C, 0x22, 0x20, 0x20, 0x26, 0x22, 0x1C, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x22, 0x22, 0x22, 0x3E, 0x22, 0x22, 0x22, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x1C, 0x08, 0x08, 0x08, 0x08, 0x08, 0x1C, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x02, 0x02, 0x02, 0x02, 0x02, 0x22, 0x1C, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x22, 0x24, 0x28, 0x30, 0x28, 0x24, 0x22, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x3E, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x22, 0x36, 0x2A, 0x2A, 0x22, 0x22, 0x22, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x22, 0x32, 0x32, 0x2A, 0x26, 0x26, 0x22, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x1C, 0x22, 0x22, 0x22, 0x22, 0x22, 0x1C, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x3C, 0x22, 0x22, 0x3C, 0x20, 0x20, 0x20, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x1C, 0x22, 0x22, 0x22, 0x2A, 0x24, 0x1A, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x3C, 0x22, 0x22, 0x3C, 0x28, 0x24, 0x22, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x1C, 0x22, 0x20, 0x1C, 0x02, 0x22, 0x1C, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x3E, 0x08, 0x08, 0x08, 0x08, 0x08, 0x08, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x1C, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x22, 0x22, 0x22, 0x14, 0x14, 0x08, 0x08, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x22, 0x22, 0x22, 0x2A, 0x2A, 0x2A, 0x14, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x22, 0x22, 0x14, 0x08, 0x14, 0x22, 0x22, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x22, 0x22, 0x14, 0x08, 0x08, 0x08, 0x08, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x3E, 0x02, 0x04, 0x08, 0x10, 0x20, 0x3E, 0x00, 0x00, // Z
    0x00, 0x00, 0x00, 0x1C, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1C, 0x00, 0x00, // [
    0x00, 0x00, 0x00, 0x20, 0x20, 0x10, 0x08, 0x04, 0x02, 0x02, 0x00, 0x00, // \
    0x00, 0x00, 0x00, 0x1C, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1C, 0x00, 0x00, // ]
    0x00, 0x00, 0x00, 0x08, 0x1C, 0x2A, 0x08, 0x08, 0x08, 0x08, 0x00, 0x00, // up
    0x00, 0x00, 0x00, 0x00, 0x08, 0x10, 0x3E, 0x10, 0x08, 0x00, 0x00, 0x00, // left
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // space
    0x00, 0x00, 0x00, 0x08, 0x08, 0x08, 0x08, 0x08, 0x00, 0x08, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x14, 0x14, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x14, 0x14, 0x3E, 0x14, 0x3E, 0x14, 0x14, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x08, 0x1E, 0x28, 0x1C, 0x0A, 0x3C, 0x08, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x32, 0x32, 0x04, 0x08, 0x10, 0x26, 0x26, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x10, 0x28, 0x28, 0x10, 0x2A, 0x24, 0x1A, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x08, 0x08, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x04, 0x08, 0x10, 0x10, 0x10, 0x08, 0x04, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x10, 0x08, 0x04, 0x04, 0x04, 0x08, 0x10, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x08, 0x2A, 0x1C, 0x2A, 0x08, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x08, 0x08, 0x3E, 0x08, 0x08, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x08, 0x10, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3E, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x02, 0x02, 0x04, 0x08, 0x10, 0x20, 0x20, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x1C, 0x22, 0x26, 0x2A, 0x32, 0x22, 0x1C, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x08, 0x18, 0x08, 0x08, 0x08, 0x08, 0x1C, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x1C, 0x22, 0x02, 0x1C, 0x20, 0x20, 0x3E, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x3E, 0x02, 0x04, 0x0C, 0x02, 0x22, 0x1C, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x04, 0x0C, 0x14, 0x24, 0x3E, 0x04, 0x04, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x3E, 0x20, 0x3C, 0x02, 0x02, 0x22, 0x1C, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x0E, 0x10, 0x20, 0x3C, 0x22, 0x22, 0x1C, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x3E, 0x02, 0x02, 0x04, 0x08, 0x10, 0x20, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x1C, 0x22, 0x22, 0x1C, 0x22, 0x22, 0x1C, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x1C, 0x22, 0x22, 0x1E, 0x02, 0x04, 0x38, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x08, 0x08, 0x10, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x06, 0x08, 0x10, 0x20, 0x10, 0x08, 0x06, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, 0x3E, 0x00, 0x3E, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x30, 0x08, 0x04, 0x02, 0x04, 0x08, 0x30, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x1C, 0x22, 0x02, 0x04, 0x08, 0x00, 0x08, 0x00, 0x00, //
];
