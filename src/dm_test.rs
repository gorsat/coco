use std::{
    sync::{atomic::AtomicBool, atomic::Ordering, Arc, Mutex, RwLock},
    thread::{self, sleep},
    time::Duration,
};

#[macro_use]
mod macros;
mod devmgr;
mod error;
mod pia;
mod registers;
mod sam;
mod sound;
mod u8oru16;
mod vdg;

pub use devmgr::*;
pub use error::*;
pub use pia::*;
pub use sam::*;
pub use u8oru16::*;
pub use vdg::*;

const MODE_CHANGE_DELAY: Duration = Duration::from_millis(200);
const SPACE_CHAR: u8 = 0x20;

fn set_sam_vdg_bits(sam: &mut Sam, bits: u8) {
    let mut mask = 1u8;
    for i in 0..3 {
        let index = if bits & mask != 0 { i * 2 + 1 } else { i * 2 };
        mask <<= 1;
        sam.write(index);
    }
}
macro_rules! vdg_println {
  ($vdg:expr,$($msg:expr),*) => {
        vdg_line_out($vdg,format!($($msg),*).as_str())
   };
}
fn clear_text_screen(vram_offset: usize) {
    let mut ram = Ram::get_self_mut().ram.as_ref().unwrap().write().unwrap();
    for i in vram_offset..vram_offset + (vdg::BLOCK_COLS * vdg::BLOCK_ROWS) {
        ram[i] = SPACE_CHAR;
    }
}
fn vdg_line_out(vdg: &Mutex<Vdg>, line: &str) {
    {
        let r = Ram::get_self();
        let c = unsafe { CURSOR };
        let p = r.ram.as_ref().unwrap().read().unwrap().as_ptr();
        verbose_println!(
            "vdg_line_out: ram={:p}, vram_offset={:x}, cursor={c:?}, msg=\"{line}\"",
            p,
            r.vram_offset
        );
        vdg.lock().unwrap().set_dirty();
    }
    for ch in line.chars() {
        vdg_char_out(ch);
    }
    vdg_char_out('\n');
}
fn vdg_char_out(ch: char) {
    unsafe {
        if CURSOR.0 >= vdg::BLOCK_COLS || ch == '\n' {
            CURSOR.0 = 0;
            CURSOR.1 += 1;
        }
        if CURSOR.1 >= vdg::BLOCK_ROWS {
            CURSOR.1 = vdg::BLOCK_ROWS - 1;
            vdg_scroll();
        }
        if ch != '\n' {
            Ram::write_vram_byte(CURSOR.0 + CURSOR.1 * vdg::BLOCK_COLS, ch as u8);
            CURSOR.0 += 1;
        }
    }
}
fn vdg_scroll() {
    for i in vdg::BLOCK_COLS..(vdg::BLOCK_COLS * vdg::BLOCK_ROWS) {
        let b = Ram::read_vram_byte(i);
        Ram::write_vram_byte(i - vdg::BLOCK_COLS, b);
    }
    for i in 0..vdg::BLOCK_COLS {
        Ram::write_vram_byte(i + vdg::BLOCK_COLS * (vdg::BLOCK_ROWS - 1), SPACE_CHAR);
    }
}
static mut CURSOR: (usize, usize) = (0, 0);

// Hack: a singleton to represent system RAM for testing
// Using this to avoid rewriting the test code within a single struct.
struct Ram {
    ram: Option<Arc<RwLock<Vec<u8>>>>,
    vram_offset: usize,
}
static mut G_RAM: Ram = Ram {
    ram: None,
    vram_offset: 0,
};
impl Ram {
    pub fn get_self() -> &'static Self { unsafe { &G_RAM } }
    pub fn get_self_mut() -> &'static mut Self { unsafe { &mut G_RAM } }
    pub fn init() { Self::get_self_mut().ram = Some(Arc::new(RwLock::new(vec![0u8; 0x2000]))); }
    pub fn get_ram_clone() -> Arc<RwLock<Vec<u8>>> { Self::get_self().ram.as_ref().unwrap().clone() }
    pub fn set_vram_offset(vram_offset: usize) { Self::get_self_mut().vram_offset = vram_offset }
    pub fn write_vram_byte(index: usize, data: u8) {
        let s = Self::get_self_mut();
        let mut ram = s.ram.as_ref().unwrap().write().unwrap();
        let addr = index + s.vram_offset;
        if addr >= ram.len() {
            panic!("VRAM write out of bounds")
        }
        ram[addr] = data;
    }
    pub fn read_vram_byte(index: usize) -> u8 {
        let s = Self::get_self();
        let ram = s.ram.as_ref().unwrap().read().unwrap();
        let addr = index + s.vram_offset;
        if addr >= ram.len() {
            panic!("VRAM read out of bounds")
        }
        ram[addr]
    }
}
fn main() {
    Ram::init();
    let mut dm = DeviceManager::with_ram(Ram::get_ram_clone(), 0x400);
    let vdg = dm.get_vdg();
    let pia0 = dm.get_pia0();
    let pia1 = dm.get_pia1();
    let sam = dm.get_sam();
    let video_tests_done = Arc::new(AtomicBool::new(false));
    let vtd = video_tests_done.clone();

    {
        vdg.lock().unwrap().interpret_chars_as_ascii(true);
    }
    thread::spawn(move || {
        let sg4 = TestArgs {
            mode: VdgMode::SG4,
            sam_bits: 0,
            pia1_bits: 0,
        };
        set_mode(&sam, &pia1, &sg4);
        set_vram_offset(&sam, 0x400);
        {
            for i in 0u8..=255 {
                Ram::write_vram_byte(i as usize, i);
            }
        }
        sleep(MODE_CHANGE_DELAY);
        let sg6 = TestArgs {
            mode: VdgMode::SG6,
            sam_bits: 0,
            pia1_bits: 0x10,
        };
        set_mode(&sam, &pia1, &sg6);
        {
            for i in 0u8..=255 {
                Ram::write_vram_byte(i as usize, i);
            }
        }
        sleep(MODE_CHANGE_DELAY);
        set_vram_offset(&sam, 0x600);
        for ta in SGX_TESTS {
            test_sgx(&sam, &pia1, ta);
            sleep(MODE_CHANGE_DELAY);
        }
        set_vram_offset(&sam, 0x200);
        for gt in G_TESTS {
            test_graphics_mode(&sam, &pia1, gt);
            sleep(MODE_CHANGE_DELAY);
        }
        set_mode(&sam, &pia1, &sg4);
        set_vram_offset(&sam, 0x400);
        clear_text_screen(0x400);
        vdg_println!(&vdg, "HELLO, WORLD!");
        vdg_println!(&vdg, "PRESS ANY KEY");
        vtd.store(true, Ordering::Release);
    });
    let vdg = dm.get_vdg();
    while dm.is_running() {
        dm.update();
        if video_tests_done.load(Ordering::Acquire) {
            let mut pia0 = pia0.lock().unwrap();
            // set pia0-b-data as all output bits
            pia0.write(2, 0xff);
            // select peripheral (not data direction) register for our writes to pia0
            pia0.write(1, 0x34);
            pia0.write(3, 0x34);
            let mut mask = 0xfeu8;
            for i in 0..8 {
                pia0.write(2, mask);
                let b = pia0.read(0);
                if b != 0xff {
                    verbose_println!("key in column {} (mask={:x}, pia0.a={:x}, com={:b})", i, mask, b, !b);
                    vdg_println!(&vdg, "KEY DOWN: COL[{}]={:8b}", i, b);
                }
                mask = mask.rotate_left(1);
            }
        }
    }
}
struct TestArgs {
    mode: VdgMode,
    sam_bits: u8,
    pia1_bits: u8,
}
const SGX_TESTS: &[TestArgs] = &[
    TestArgs {
        mode: VdgMode::SG8,
        sam_bits: 2,
        pia1_bits: 0,
    },
    TestArgs {
        mode: VdgMode::SG12,
        sam_bits: 4,
        pia1_bits: 0,
    },
    TestArgs {
        mode: VdgMode::SG24,
        sam_bits: 6,
        pia1_bits: 0,
    },
];
const G_TESTS: &[TestArgs] = &[
    TestArgs {
        mode: VdgMode::CG1,
        sam_bits: 0x01,
        pia1_bits: 0x88,
    },
    TestArgs {
        mode: VdgMode::RG1,
        sam_bits: 0x01,
        pia1_bits: 0x90,
    },
    TestArgs {
        mode: VdgMode::CG2,
        sam_bits: 0x02,
        pia1_bits: 0xa0,
    },
    TestArgs {
        mode: VdgMode::RG2,
        sam_bits: 0x03,
        pia1_bits: 0xb0,
    },
    TestArgs {
        mode: VdgMode::CG3,
        sam_bits: 0x04,
        pia1_bits: 0xc0,
    },
    TestArgs {
        mode: VdgMode::RG3,
        sam_bits: 0x05,
        pia1_bits: 0xd0,
    },
    TestArgs {
        mode: VdgMode::CG6,
        sam_bits: 0x06,
        pia1_bits: 0xe0,
    },
    TestArgs {
        mode: VdgMode::RG6,
        sam_bits: 0x06,
        pia1_bits: 0xf0,
    },
];
fn set_mode(sam: &Mutex<Sam>, pia1: &Mutex<Pia1>, ta: &TestArgs) {
    //todo! set DDR bit to target PR here!
    println!("setting mode: {:?}", ta.mode);
    let mut sam = sam.lock().unwrap();
    let mut pia1 = pia1.lock().unwrap();
    set_sam_vdg_bits(&mut sam, ta.sam_bits);
    // make sure pia1-b data register bits are all outputs
    pia1.write(3, 0);
    pia1.write(2, 0xff);
    // select peripheral register as destination for data write to PIA1B
    pia1.write(3, 4);
    // write VDG config
    let b = pia1.read(2);
    pia1.write(2, ta.pia1_bits | (b & 7));
}
fn set_vram_offset(sam: &Mutex<Sam>, vram_offset: usize) {
    println!("setting vram_offset: {:x}", vram_offset);
    Ram::set_vram_offset(vram_offset);
    let mut sam = sam.lock().unwrap();
    let mut bits = vram_offset / 512;
    for i in 3..=9usize {
        let index = i * 2 + (bits & 1);
        sam.write(index);
        bits >>= 1;
    }
}
fn test_sgx(sam: &Mutex<Sam>, pia1: &Mutex<Pia1>, ta: &TestArgs) {
    set_mode(sam, pia1, ta);
    let md = ta.mode.get_details();
    let cell_rows = vdg::BLOCK_DIM_Y / md.cell_y;
    for i in 0usize..256 {
        let block_row = i / vdg::BLOCK_COLS;
        for cell_row in 0usize..cell_rows {
            let dst_index = (block_row * cell_rows + cell_row) * vdg::BLOCK_COLS + i % vdg::BLOCK_COLS;
            let data = if i < 0x80 { i } else { 0x80 | ((i + cell_row) & 0xff) };
            Ram::write_vram_byte(dst_index, data as u8);
        }
    }
}
fn test_graphics_mode(sam: &Mutex<Sam>, pia1: &Mutex<Pia1>, ta: &TestArgs) {
    set_mode(sam, pia1, ta);
    draw_rect(ta.mode);
}
fn draw_rect(mode: VdgMode) {
    let md = mode.get_details();
    // paint the whole screen background color
    let cells_per_byte = 8 / md.color_bits;
    for row in 0..(SCREEN_DIM_Y / md.cell_y) {
        let cols_per_row = SCREEN_DIM_X / (md.cell_x * cells_per_byte);
        for col in 0..cols_per_row {
            let dst_index = row * cols_per_row + col;
            Ram::write_vram_byte(dst_index, 0);
        }
    }
    // 10 cells x 10 cells
    let rect = (10, 10);
    let cell_mid_x = (vdg::SCREEN_DIM_X / md.cell_x) / 2;
    let cell_mid_y = (vdg::SCREEN_DIM_Y / md.cell_y) / 2;
    let bytes_per_row = SCREEN_DIM_X / (md.cell_x * cells_per_byte);
    for cell_col in (cell_mid_x - rect.0 / 2)..(cell_mid_x + rect.0 / 2) {
        for cell_row in (cell_mid_y - rect.1 / 2)..(cell_mid_y + rect.1 / 2) {
            // set the corresponding src bits for this cell
            let byte_col = cell_col / cells_per_byte;
            let bit_index = md.color_bits * (cell_col % cells_per_byte);
            let mut mask = if md.color_bits == 2 { 0b11000000u8 } else { 0b10000000u8 };
            mask >>= bit_index;
            let dst_index = cell_row * bytes_per_row + byte_col;
            let data = Ram::read_vram_byte(dst_index) | mask;
            Ram::write_vram_byte(dst_index, data);
        }
    }
}
