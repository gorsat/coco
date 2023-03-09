pub trait Pia {
    fn read(&mut self, reg_num: usize) -> u8;
    fn write(&mut self, reg_num: usize, data: u8);
}

/// Implements one "side" of a PIA chip
#[derive(Debug, Default)]
struct PiaSide {
    // control register
    cr: u8,
    // peripheral register
    ir: u8,
    // output register
    or: u8,
    // data direction register
    ddr: u8,
    // control lines
    c1: bool,
    c2: bool,
}

#[allow(unused)]
impl PiaSide {
    fn manual_c2_trigger(&self) -> bool { self.cr & 0x30 == 0x30 }
    fn write_control(&mut self, b: u8) {
        // bits 6 & 7 are read-only
        self.cr = (b & 0x3f) | (self.cr & 0xc0);
        // check for setting c2 (when it is configured as an output)
        if self.manual_c2_trigger() {
            // bit 3 controls c2
            self.set_c2(self.cr & 8 == 8);
        }
    }
    fn read_control(&mut self) -> u8 {
        let b = self.cr;
        // control line flags are cleared on read
        self.cr &= 0x3f;
        b
    }
    fn pr_selected(&self) -> bool { self.cr & 4 == 4 }
    fn write(&mut self, index: usize, b: u8) {
        if index & 1 == 1 {
            self.write_control(b)
        } else {
            self.write_data(b)
        }
    }
    fn read(&mut self, index: usize) -> u8 {
        if index & 1 == 1 {
            self.read_control()
        } else {
            self.read_data()
        }
    }
    fn write_data(&mut self, b: u8) {
        // bit 2 in CR determines which register receives the write
        if self.pr_selected() {
            // write to peripheral register
            self.or = b & self.ddr;
        } else {
            // write to DDR
            self.ddr = b;
        }
    }
    fn read_data(&self) -> u8 {
        if self.pr_selected() {
            // bits marked as outputs source from output register
            // bits marked as inputs source from the peripheral register
            self.or & self.ddr | self.ir & !self.ddr
        } else {
            // configured for data register == data direction register
            // just return the contents of DDR
            self.ddr
        }
    }
    fn read_output(&self) -> u8 {
        // ddr controls which bits of the output register are seen
        (self.or & self.ddr) | !self.ddr
    }
    fn set_c1(&mut self, c1: bool) {
        // Note! only supporting low-high transitions
        if c1 && !self.c1 {
            // set c1 flag; (bit 7)
            self.cr |= 0x80;
        }
        self.c1 = c1;
    }
    fn set_c2(&mut self, c2: bool) {
        // Note only supporting low-high transitions
        if c2 && !self.c2 && !self.manual_c2_trigger() {
            // set c2 flag; (bit 6)
            self.cr |= 0x40;
            // remember this transition
        }
        self.c2 = c2;
    }
    // returns true if an interrupt signal is active
    // and resets the interrupt to inactive
    fn consume_interrupt(&mut self) -> bool {
        let mut interrupt = false;
        // if control line 1 transitioned and interrupt from c1 is enabled in cr...
        if self.c1 && (self.cr & 1 == 1) {
            interrupt = true;
            self.c1 = false;
        }
        // if control line 2 transitioned and interrupt from c2 is enabled in cr...
        // AND control line 2 is configured as an input in cr...
        if self.c2 && (self.cr & 0x28 == 0x8) {
            interrupt = true;
            self.c2 = false;
        }
        interrupt
    }
}

use std::{
    collections::HashMap,
    sync::{mpsc, Arc, Mutex},
};

/// Keyboard map for coco (from [worldofdragon.org](https://worldofdragon.org/index.php?title=Keyboard))
///       LSB              $FF02                    MSB
///     | PB0   PB1   PB2   PB3   PB4   PB5   PB6   PB7 <- column
/// ----|----------------------------------------------
/// PA0 |   @     A     B     C     D     E     F     G    LSB
/// PA1 |   H     I     J     K     L     M     N     O     $
/// PA2 |   P     Q     R     S     T     U     V     W     F
/// PA3 |   X     Y     Z    Up  Down  Left Right Space     F
/// PA4 |   0     1!    2"    3#    4$    5%    6&    7'    0
/// PA5 |   8(    9)    :*    ;+    ,<    -=    .>    /?    0
/// PA6 | ENT   CLR   BRK   N/C   N/C   N/C   N/C  SHFT
/// PA7 - Comparator input                                 MSB
///  ^
///  |
/// row
///
/// The color computer keyboard was setup differently than a standard, modern US keyboard.
/// The following mappings are required (from modern US keyboard to coco key matrix):
/// [a single modern key maps to one or more different coco keys]
///    esc             --> BRK       == (6,2)
///    home            --> CLR       == (6,1)
///    bkspc           --> Left      == (3,5)
///    '''             --> shift-'7' == [(6,7),(4,7)]
///    '='             --> shift-'-' == [(6,7),(5,5)]
/// [a shifted modern key maps to one or more different coco keys]
///    '@' (shift-'2') --> '@'       == (0,0)
///    ':' (shift-';') --> ':'       == (5,2)
///    '"' (shift-''') --> shift-'2' == [(6,7),(4,2)]
///    '&' (shift-'7') --> shift-'6' == [(6,7),(4,6)]
///    '*' (shift-'8') --> shift-':' == [(6,7),(5,2)]
///    '(' (shift-'9') --> shift-'8' == [(6,7),(5,0)]
///    ')' (shift-'0') --> shift-'9' == [(6,7),(5,1)]
///    '+' (shift-'=') --> shift-';' == [(6,7),(5,3)]
///
use minifb::{Key, MouseButton, MouseMode};

use crate::{sound::AudioSample, vdg};
#[derive(Debug)]
struct KeyMap {
    from: Key,
    to: &'static [(usize, usize)],
}
// keys from modern keyboard that didn't exist on coco
#[rustfmt::skip]
static ONE_TO_N: &[KeyMap] = &[
    KeyMap {from: Key::Backspace, to: &[(3,5)]},
    KeyMap {from: Key::LeftShift, to: &[(6,7)]},
    KeyMap {from: Key::Apostrophe, to: &[(6,7),(4,7)]},
    KeyMap {from: Key::Equal, to: &[(6,7),(5,5)]},
];
// shift+key combos from modern keyboard that don't match coco's mapping
#[rustfmt::skip]
static SHIFT_ONE_TO_N: &[KeyMap] = &[
    KeyMap {from:Key::Key2, to:&[(0,0)]},
    KeyMap {from:Key::Semicolon, to:&[(5,2)]},
    KeyMap {from:Key::Apostrophe, to:&[(6,7),(4,2)]},
    KeyMap {from:Key::Key7, to:&[(6,7),(4,6)]},
    KeyMap {from:Key::Key8, to:&[(6,7),(5,2)]},
    KeyMap {from:Key::Key9, to:&[(6,7),(5,0)]},
    KeyMap {from:Key::Key0, to:&[(6,7),(5,1)]},
    KeyMap {from:Key::Equal, to:&[(6,7),(5,3)]},
];
///
/// Note: Both LeftShift and RightShift map to SHFT
///
#[rustfmt::skip]
const KEY_MATRIX: &[[minifb::Key;8];8] = &[
    [Key::Unknown /* @ */, Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G],
    [Key::H, Key::I, Key::J, Key::K, Key::L, Key::M, Key::N, Key::O],
    [Key::P, Key::Q, Key::R, Key::S, Key::T, Key::U, Key::V, Key::W],
    [Key::X, Key::Y, Key::Z, Key::Up, Key::Down, Key::Left, Key::Right, Key::Space],
    [Key::Key0, Key::Key1, Key::Key2, Key::Key3, Key::Key4, Key::Key5, Key::Key6, Key::Key7],
    [Key::Key8, Key::Key9, Key::Unknown /* : */, Key::Semicolon, Key::Comma, Key::Minus, Key::Period, Key::Slash],
    [Key::Enter, Key::Home /* CLR */, Key::Escape /* BRK */, Key::Unknown, Key::Unknown, Key::Unknown, Key::Unknown, Key::RightShift],
    [Key::Unknown, Key::Unknown, Key::Unknown, Key::Unknown, Key::Unknown, Key::Unknown, Key::Unknown, Key::Unknown],
];
#[derive(Debug)]
pub struct Pia0 {
    ab: [PiaSide; 2],
    col: [u8; 8],
    direct_map: HashMap<minifb::Key, Vec<(usize, usize)>>,
    shift_map: HashMap<minifb::Key, Vec<(usize, usize)>>,
    joy_x: u8,
    joy_y: u8,
    joy_sw_1: bool,
    joy_sw_2: bool,
    // Deadlock risk! but Pia0 needs to read Pia1.
    // In real life, they are wired together.
    // I'm sure there's a better way to do this
    // but it will have to wait.
    pia1: Arc<Mutex<Pia1>>,
}
impl Pia for Pia0 {
    fn read(&mut self, reg_num: usize) -> u8 {
        let i = reg_num % 4;
        if i == 0 {
            // caller is reading pia0.a data
            // In order to set bit 7 appropriately we need to
            // compare the value of the DAC with the selected joystick.
            // Note: we route the mouse to BOTH joysticks
            let joy_val = match self.ab[0].c2 {
                // horizontal axis
                false => self.joy_x,
                // vertical axis
                true => self.joy_y,
            };
            // DAC val is in the top 6 bits of A side data register of pia1
            // This is the only reason we need a reference to pia1 here.
            // We must get the latest value and can't use any kind of caching.
            let dac = {
                let mut pia1 = self.pia1.lock().unwrap();
                pia1.read(0) >> 2
            };
            if dac > joy_val {
                // clear comparitor flag
                self.ab[0].ir &= 0x7f;
            } else {
                // set comparitor flag
                self.ab[0].ir |= 0x80;
            }
        }
        self.ab[(i >> 1) & 1].read(reg_num)
    }
    fn write(&mut self, reg_num: usize, data: u8) {
        let i = reg_num % 4;
        self.ab[(i >> 1) & 1].write(i, data);
        match i {
            // if write is to one of the control registers then check DAC mux bits
            1 | 3 => self.pia1.lock().unwrap().set_dac_mux(self.ab[0].c2, self.ab[1].c2),
            // if write is to the b-side data register, then it's related to keyboard
            2 => self.strobe_keyboard(),
            _ => (),
        }
    }
}
impl Pia0 {
    #[allow(clippy::new_without_default)]
    pub fn new(pia1: Arc<Mutex<Pia1>>) -> Self {
        let mut direct_map: HashMap<minifb::Key, Vec<(usize, usize)>> = HashMap::new();
        // add our KEY_MATRIX entries to the direct_map
        #[allow(clippy::needless_range_loop)]
        for row in 0..8usize {
            for col in 0..8usize {
                direct_map.insert(KEY_MATRIX[row][col], vec![(row, col); 1]);
            }
        }
        // add our ONE_TO_N entries to the direct_map
        ONE_TO_N.iter().for_each(|m| {
            direct_map.insert(m.from, m.to.to_vec());
        });
        // now populate the shift_map with entries from SHIFT_ONE_TO_N
        let mut shift_map: HashMap<minifb::Key, Vec<(usize, usize)>> = HashMap::new();
        SHIFT_ONE_TO_N.iter().for_each(|m| {
            shift_map.insert(m.from, m.to.to_vec());
        });
        Pia0 {
            ab: [PiaSide::default(), PiaSide::default()],
            col: [0xff; 8],
            direct_map,
            shift_map,
            joy_x: 0x1f,
            joy_y: 0x1f,
            joy_sw_1: false,
            joy_sw_2: false,
            pia1,
        }
    }
    // update is called periodically to allow for updates of keyboard and joystick state
    pub fn update(&mut self, w: &minifb::Window) {
        self.update_keyboard(w);
        self.update_joystick(w);
    }
    fn update_joystick(&mut self, w: &minifb::Window) {
        if let Some(mouse) = w.get_mouse_pos(MouseMode::Clamp) {
            // translate mouse position into 6-bit integers
            self.joy_x = ((255.0 * (mouse.0 / vdg::SCREEN_DIM_X as f32)).round() as u8) >> 2;
            self.joy_y = ((255.0 * (mouse.1 / vdg::SCREEN_DIM_Y as f32)).round() as u8) >> 2;
            self.joy_sw_1 = w.get_mouse_down(MouseButton::Left);
            self.joy_sw_2 = w.get_mouse_down(MouseButton::Right);
        } 
    }
    fn update_keyboard(&mut self, w: &minifb::Window) {
        let mut coords: Vec<(usize, usize)> = Vec::new();
        let keys = w.get_keys();
        // clear out our internal keyboard matrix
        for c in self.col.iter_mut() {
            *c = 0
        }
        if !keys.is_empty() {
            let shift = keys.iter().any(|&k| k == Key::LeftShift || k == Key::RightShift);
            if shift {
                // shift key is down; check shift_map to see if there are any matches
                // if so then the 1st match will be the only key press we report (any other keys will be ignored)
                if let Some(v) = keys.iter().find_map(|k| self.shift_map.get(k)) {
                    v.iter().for_each(|&c| coords.push(c));
                }
            }
            if coords.is_empty() {
                // shift key is not down or we didn't find a shift+key mapping
                // so now we just try to use a direct mapping of each of the keypresses
                keys.iter().for_each(|k| {
                    if let Some(v) = self.direct_map.get(k) {
                        v.iter().for_each(|&c| coords.push(c));
                    }
                });
            }
            // now set each column in the matrix based on the new (row,col) coords
            coords.iter().for_each(|&(r, c)| self.col[c] |= 1 << r as u8);
        }
        self.strobe_keyboard()
    }
    pub fn strobe_keyboard(&mut self) {
        // strobe the keyboard based on side B output
        let mut com = 0u8;
        // use read_output because data direction settings matter here
        let mut cols = !self.ab[1].read_output();
        if cols != 0 {
            // info!("strobing with {cols:08b}");
            for i in 0..8 {
                if cols & 1 == 1 {
                    // strobing column i
                    com |= self.col[i];
                }
                cols >>= 1;
            }
        }
        // handle joystick switches -- both joysticks mapped to the mouse
        if self.joy_sw_1 {
            // only provide joystick switch if caller didn't strobe associated col(s)
            com |= 0x3 & !cols
        }
        if self.joy_sw_2 {
            // only provide joystick switch if caller didn't strobe associated col(s)
            com |= 0xc & !cols
        }
        // store the result of strobing in the side A input register
        self.ab[0].ir = !com;
    }
    // fires the hsync hw interrupt into pia0 and then checks to see if an IRQ should result
    pub fn hsync_irq(&mut self) -> bool {
        self.ab[0].set_c1(true);
        self.ab[0].consume_interrupt()
    }
    // fires the vsync hw interrupt into pia0 and then checks to see if an IRQ should result
    pub fn vsync_irq(&mut self) -> bool {
        self.ab[1].set_c1(true);
        self.ab[1].consume_interrupt()
    }
}
#[derive(Debug)]
pub struct Pia1 {
    ab: [PiaSide; 2],
    sndr: mpsc::Sender<AudioSample>,
    sound_enabled: bool,
    dac_sel_a: bool,
    dac_sel_b: bool,
    last_bit_sound: bool,
}
impl Pia for Pia1 {
    fn read(&mut self, reg_num: usize) -> u8 { self.ab[(reg_num >> 1) & 1].read(reg_num) }
    fn write(&mut self, reg_num: usize, data: u8) {
        let i = reg_num % 4;
        self.ab[(i >> 1) & 1].write(reg_num, data);
        
        // handle pia1-specific functionality
        match i {
            0 if self.sound_enabled && !self.dac_sel_a && !self.dac_sel_b => {
                // this is a write to the DAC and sound is enabled so send the data to the audio device
                // convert 6-bit amplitude into f32 value between -1.0 and +1.0
                let fdata = ((self.ab[0].read_output() >> 2) as f32 - 31.0) / 32.0;
                self.sndr
                    .send(AudioSample::new(fdata))
                    .expect("error sending audio sample to channel");
            }
            2 => {
                // check for single-bit sound in pia1-b data register
                let bit = self.ab[1].read_output() & 2 == 2;
                if bit != self.last_bit_sound {
                    let fdata = if bit { 0.5 } else { -0.5 };
                    self.sndr
                        .send(AudioSample::new(fdata))
                        .expect("error sending single bit audio to channel")
                }
                self.last_bit_sound = bit;
            }
            3 => self.sound_enabled = data & 8 == 8,
            _ => (),
        }
    }
}
impl Pia1 {
    pub fn new(sndr: mpsc::Sender<AudioSample>) -> Self {
        Pia1 {
            ab: [PiaSide::default(), PiaSide::default()],
            sndr,
            sound_enabled: false,
            dac_sel_a: false,
            dac_sel_b: false,
            last_bit_sound: false,
        }
    }
    /// Returns the following bits as a byte: 0, 0, 0, G/!A, GM2, GM1, GM0, CSS
    pub fn get_vdg_bits(&self) -> u8 { (self.ab[1].read_data() >> 3) & 0x1f }
    /// Lets PIA1 know that a cartridge was inserted.
    /// Returns true if FIRQ is signalled
    pub fn cart_firq(&mut self) -> bool {
        self.ab[1].set_c1(true);
        self.ab[1].consume_interrupt()
    }
    pub fn set_dac_mux(&mut self, a: bool, b: bool) {
        self.dac_sel_a = a;
        self.dac_sel_b = b;
    }
}
