#![allow(unused)]
/// Simple interface for reading and writing the Synchronous Address Multiplexer
/// Sam Control Register bit definitions:
/// Bits  | Usage
/// 0-2   | VDG Addressing Mode (sets VDG mode in combination with VDG bits)
/// 3-9   | VDG Address Offset (the start of VRAM in system memory)
/// 10    | Page Switch (used for 64K addressing -- not used in coco 1&2)
/// 11-12 | Clock Speed
/// 13-14 | Memory Size
/// 15    | Map Type (ROM+RAM or RAM-only; coco uses ROM+RAM)
///
#[derive(Debug)]
pub struct Sam {
    config: u16,
}

impl Sam {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self { Sam { config: 0 } }
    pub fn get_raw_config(&self) -> u16 { self.config }
    pub fn get_vdg_bits(&self) -> u8 { VDG_MODE.from_config(self.config) as u8 }
    pub fn get_vram_start(&self) -> u16 { 512 * VRAM_START.from_config(self.config) }
    pub fn get_page_switch(&self) -> bool { (PAGE_SWITCH.from_config(self.config)) != 0 }
    pub fn get_mpu_rate(&self) -> u8 { MPU_RATE.from_config(self.config)as u8 }
    pub fn get_map_type(&self) -> bool { MAP_TYPE.from_config(self.config) != 0 }
    pub fn write(&mut self, index: usize) {
        if index >= 32 {
            panic!()
        }
        let mut val = 1u16 << (index / 2);
        if index & 1 == 0 {
            self.config &= !val;
        } else {
            self.config |= val;
        }
        verbose_println!("SAM config={:016b}",self.config);
    }
}

struct SamBits {
    mask: u16,
    offset: u16,
}
#[allow(clippy::wrong_self_convention)]
impl SamBits {
    #[inline(always)]
    fn from_config(&self, config: u16) -> u16 { (config & self.mask) >> self.offset }
}
const VDG_MODE: SamBits = SamBits {
    mask: 0x0007,
    offset: 0,
};
const VRAM_START: SamBits = SamBits {
    mask: 0x03F8,
    offset: 3,
};
const PAGE_SWITCH: SamBits = SamBits {
    mask: 0x400,
    offset: 10,
};
const MPU_RATE: SamBits = SamBits {
    mask: 0x1800,
    offset: 11,
};
const MEM_SIZE: SamBits = SamBits {
    mask: 0x6000,
    offset: 14,
};
const MAP_TYPE: SamBits = SamBits {
    mask: 0x8000,
    offset: 15,
};
