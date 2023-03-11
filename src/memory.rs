use crate::pia::Pia;

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]

pub enum AccessType {
    Program,
    UserStack,
    SystemStack,
    Generic,
    System,
}

impl Core {
    // reads one byte from RAM
    #[inline(always)]
    pub fn _read_u8(&self, _: AccessType, addr: u16, data: Option<&mut u8>) -> Result<u8, Error> {
        // first check to see if this address is overridden by the ACIA
        if let Some(acia) = self.acia.as_ref() {
            if acia.owns_address(addr) {
                return acia.read(addr);
            }
        }
        // if the debugger is enabled then check to see if this read should trigger a breakpoint
        if config::debug() {
            self.debug_check_for_watch_hit(addr);
        }
        let byte = match addr {
            0x0000..=0xfeff => {
                // the address is within the address space of RAM/ROM
                // just complete the read from memory
                self.raw_ram[addr as usize]
            }
            0xff00..=0xff1f => {
                // pia0
                let mut pia = self.pia0.lock().unwrap();
                pia.read((addr - 0xff00) as usize)
            }
            0xff20..=0xff3f => {
                // pia1
                let mut pia = self.pia1.lock().unwrap();
                pia.read((addr - 0xff20) as usize)
            }
            0xffc0..=0xffdf => {
                // sam (write-only)
                0u8
            }
            0xffe0..=0xffff => {
                // remap interrupt vectors to 0xbfe0-0xbfff
                self.raw_ram[(addr - 0x4000) as usize]
            }
            _ => {
                warn!("Read at unimplemented addres {:04x}", addr);
                0
            }
        };
        if let Some(data) = data {
            *data = byte;
        }
        Ok(byte)
    }
    // helper version of _read_u8 that reads a byte into a u16
    #[inline(always)]
    pub fn _read_u8_as_u16(&self, atype: AccessType, addr: u16, data: Option<&mut u16>) -> Result<u16, Error> {
        let byte = self._read_u8(atype, addr, None)?;
        let word = byte as u16;
        if let Some(data) = data {
            *data = word
        }
        Ok(word)
    }
    // version of _read... for u16
    // reads two bytes as a u16 (high order byte first)
    #[inline(always)]
    pub fn _read_u16(&self, atype: AccessType, addr: u16, data: Option<&mut u16>) -> Result<u16, Error> {
        let mut b: [u8; 2] = [0, 0];
        self._read_u8(atype, addr, Some(&mut b[0]))?;
        self._read_u8(atype, addr + 1, Some(&mut b[1]))?;
        let word = (b[0] as u16) << 8 | (b[1] as u16);
        if let Some(data) = data {
            *data = word;
        }
        Ok(word)
    }
    // version of _read... for u8u16
    #[inline(always)]
    pub fn _read_u8u16(&self, atype: AccessType, addr: u16, size: u16) -> Result<u8u16, Error> {
        match size {
            1 => {
                let b = self._read_u8(atype, addr, None)?;
                Ok(u8u16::u8(b))
            }
            2 => {
                let w = self._read_u16(atype, addr, None)?;
                Ok(u8u16::u16(w))
            }
            _ => panic!("invalid read size for _read_u8u16"),
        }
    }
    //
    // writes
    //
    #[inline(always)]
    pub fn _write_u8(&mut self, at: AccessType, addr: u16, data: u8) -> Result<(), Error> {
        // first check to see if this address is overridden by the ACIA
        if let Some(acia) = self.acia.as_mut() {
            if acia.owns_address(addr) {
                return acia.write(addr, data);
            }
        }
        // if the debugger is enabled then check to see if this write should trigger a breakpoint
        if config::debug() {
            self.debug_check_for_watch_hit(addr);
        }
        match addr {
            0x0000..=0xfeff => {
                if addr > self.ram_top && at != AccessType::System {
                    // if the address of the write is in ROM and the write is from regular code then ignore it
                    return Ok(());
                }
                // the address is within the address space of RAM
                self.raw_ram[addr as usize] = data;
            }
            0xff00..=0xff1f => {
                // pia0
                let mut pia = self.pia0.lock().unwrap();
                pia.write((addr - 0xff00) as usize, data);
            }
            0xff20..=0xff3f => {
                // pia1
                let mut pia = self.pia1.lock().unwrap();
                pia.write((addr - 0xff20) as usize, data);
            }
            0xffc0..=0xffdf => {
                // sam
                let mut sam = self.sam.lock().unwrap();
                sam.write((addr - 0xffc0) as usize);
            }
            0xffe0..=0xffff => {
                if addr > self.ram_top && at != AccessType::System {
                // if the address of the write is in ROM and the write is from regular code then ignore it
                    return Ok(());
                }
                // remap interrupt vectors to 0xbfe0-0xbfff
                self.raw_ram[(addr-0x4000) as usize] = data;
            }
            _ => warn!("Write at unimplemented address {:04x}", addr),
        }
        Ok(())
    }
    #[inline(always)]
    pub fn _write_u8u16(&mut self, atype: AccessType, addr: u16, data: u8u16) -> Result<(), Error> {
        let mut offset = 0u16;
        if let Some(msb) = data.msb() {
            self._write_u8(atype, addr, msb)?;
            offset += 1;
        }
        self._write_u8(atype, addr + offset, data.lsb())
    }
}
