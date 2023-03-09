use std::time::Duration;

/// Implements the runtime engine of the simulator.
use crate::{
    core::InterruptType,
    instructions::{PPPostByte, TEPostByte},
};

use super::*;
use memory::AccessType;

pub const HSYNC_PERIOD: Duration = Duration::from_nanos(63_500);
pub const VSYNC_PERIOD: Duration = Duration::from_micros(16_667);

impl Core {
    /// Resets the 6809 by clearing the registers and
    /// then loading the program counter from the reset vector
    /// (or using the override value if one has been set)
    pub fn reset(&mut self) -> Result<(), Error> {
        self.reg.reset();
        if let Some(addr) = self.reset_vector {
            self.force_reset_vector(addr)?
        }
        self.reg.pc = self._read_u16(memory::AccessType::System, 0xfffe, None)?;
        self.program_start = self.reg.pc;
        self.faulted = false;
        Ok(())
    }
    /// Writes the given address to the reset vector
    pub fn force_reset_vector(&mut self, addr: u16) -> Result<(), Error> {
        self._write_u8u16(memory::AccessType::System, 0xfffe, u8u16::u16(addr))
    }
    /// Displays current perf information to stdout
    #[allow(dead_code)]
    fn report_perf(&self) {
        if !config::ARGS.perf {
            return;
        }
        let total_time = self.start_time.elapsed();
        info!(
            "Executed {} instructions in {:.2} sec; {:.3} MIPS; effective clock: {:.3} MHz",
            self.instruction_count,
            total_time.as_secs_f32(),
            self.instruction_count as f32 / (total_time.as_secs_f32() * 1.0e6),
            self.clock_cycles as f32 / (total_time.as_secs_f32() * 1.0e6)
        );
        info!("\t{:<10} {:>6} {:>5}", "Phase", "Time", "%");
        info!("\t-----------------------");
        macro_rules! perf_row {
            ($name:expr, $id:expr) => {
                info!(
                    "\t{:<10} {:>6.3} {:>5.1}",
                    $name,
                    $id.as_secs_f32(),
                    100.0 * $id.as_secs_f32() / total_time.as_secs_f32()
                )
            };
        }
        perf_row!("meta", self.meta_time);
        perf_row!("prep", self.prep_time);
        perf_row!("eval", self.eval_time);
        // let read_time = self.read_time.get();
        // perf_row!("read", read_time);
        // perf_row!("write", self.write_time);
        perf_row!("commit", self.commit_time);
        perf_row!("total", total_time);
    }
    /// Starts executing instructions at the current program counter.  
    /// Does not set or read any registers before attempting to execute.  
    /// Will attempt to execute until a SWI* instruction or a fault is encountered.
    /// A normal exit results in Ok; anything else results in Err.
    pub fn exec(&mut self) -> Result<(), Error> {
        self.start_time = Instant::now();
        loop {
            let temp_pc = self.reg.pc;
            if let Err(e) = self.exec_one() {
                if e.kind == ErrorKind::Exit {
                    // this is a normal exit
                    break;
                }
                // if the debugger is disabled then stop executing and return the error
                // otherwise, the debug cli will be invoked when we try to exec the next instruction (due to the fault)
                if !config::debug() {
                    return Err(e);
                } else {
                    self.fault(temp_pc, &e);
                }
            }
            if let Some(time) = config::ARGS.time {
                if self.start_time.elapsed() > Duration::from_secs_f32(time) {
                    info!("Terminating because the specified time has expired.");
                    break;
                }
            }
        }
        if config::ARGS.perf {
            self.report_perf()
        }
        Ok(())
    }
    /// Helper function for exec.  
    /// Wraps calls to exec_next and adds debug checks and interrupt processing.
    fn exec_one(&mut self) -> Result<(), Error> {
        let function_start = Instant::now();
        let mut meta_start: Option<Instant> = None;
        let mut expected_duration: Option<Duration> = None;
        if config::debug() && self.pre_instruction_debug_check(self.reg.pc) {
            self.debug_cli()?;
        }
        let temp_pc = self.reg.pc;
        if !self.in_cwai && !self.in_sync {
            let outcome = self.exec_next(self.list_mode.is_none())?;
            meta_start = Some(Instant::now());
            // if paying attention to timing then track how long this instruction should have taken
            expected_duration = self
                .min_cycle
                .and_then(|min| min.checked_mul(outcome.inst.flavor.detail.clk as u32));
            // if let Some(expected) = expected_duration {
            //     if function_start.elapsed() > expected * 100 {
            //         warn!(
            //             "instruction {} at {:04x} too slow: {} usec, should be {} usec",
            //             outcome.inst.flavor.desc.name,
            //             outcome.inst.ctx.pc,
            //             function_start.elapsed().as_micros(),
            //             expected.as_micros()
            //         );
            //         info!("{:?}",outcome.inst.flavor.desc);
            //     }
            // }
            // check for meta instructions (SWIx, SYNC, CWAI)
            if let Some(meta) = outcome.meta.as_ref() {
                let it = meta.to_interrupt_type();
                match meta {
                    instructions::Meta::EXIT => {
                        info!("EXIT instruction at PC={:0x}", self.reg.pc);
                        return Err(Error::new(
                            ErrorKind::Exit,
                            None,
                            "program terminated by EXIT instruction",
                        ));
                    }
                    instructions::Meta::CWAI => {
                        self.stack_for_interrupt(true)?;
                        self.in_cwai = true;
                        verbose_println!("CWAI at PC={:0x}: waiting for interrupt...", self.reg.pc);
                    }
                    instructions::Meta::SYNC => {
                        self.in_sync = true;
                        verbose_println!("SYNC at PC={:0x}: waiting for interrupt...", self.reg.pc);
                    }
                    _ if it.is_some() => {
                        self.start_interrupt(it.unwrap())?;
                    }
                    _ => {
                        panic!("meta-instruction {:?} not supported", meta);
                    }
                }
            }
            if config::help_humans() {
                self.post_instruction_debug_check(temp_pc, &outcome);
            }
        }
        if meta_start.is_none() {
            meta_start = Some(Instant::now());
        }
        let mut irq;
        let mut firq = false;
        // check for work that needs to be done on hsync
        if self.hsync_prev.elapsed() >= HSYNC_PERIOD {
            self.hsync_prev = Instant::now();
            // check for hardware firq
            {
                let mut pia1 = self.pia1.lock().unwrap();
                if self.cart_pending {
                    firq = pia1.cart_firq();
                }
            }
            // check for hardware irq
            {
                let mut pia0 = self.pia0.lock().unwrap();
                irq = pia0.hsync_irq();
            }
            // if it's vsync time, then also check for vsync irq
            if self.vsync_prev.elapsed() >= VSYNC_PERIOD {
                self.vsync_prev = Instant::now();
                {
                    let mut pia0 = self.pia0.lock().unwrap();
                    irq = irq || pia0.vsync_irq();
                }
            }
            if irq {
                // hardware issued an hsync irq
                // sync completes whether or not we service the interrupt
                self.in_sync = false;
                // if irq is not masked then service it
                if !self.reg.cc.is_set(registers::CCBit::I) {
                    self.start_interrupt(InterruptType::Irq)?;
                }
            }
            if firq {
                // hardware issued a firq
                // sync completes whether or not we service the interrupt
                self.in_sync = false;
                // if FIRQ is not masked then service it
                if !self.reg.cc.is_set(registers::CCBit::F) {
                    self.start_interrupt(InterruptType::Firq)?;
                    self.cart_pending = false;
                }
            }
        }
        // finally check to make sure we didn't execute this instruction too quickly
        if let Some(remaining_time) = expected_duration.and_then(|m| m.checked_sub(function_start.elapsed())) {
            let time = Instant::now();
            while Instant::now() - time < remaining_time { /* spin */ }
        }
        self.meta_time += meta_start.unwrap().elapsed();
        Ok(())
    }

    // helper function for interrupt handling
    pub fn system_psh(&mut self, reg: registers::Name) -> Result<(), Error> {
        let mut addr = self.reg.get_register(registers::Name::S).u16();
        if addr < registers::reg_size(reg) {
            return Err(runtime_err!(Some(self.reg), "interal_push stack overflow"));
        }
        addr -= registers::reg_size(reg);
        self._write_u8u16(AccessType::System, addr, self.reg.get_register(reg))?;
        self.reg.set_register(registers::Name::S, u8u16::u16(addr));
        Ok(())
    }
    pub fn stack_for_interrupt(&mut self, entire: bool) -> Result<(), Error> {
        // save the appropriate registers
        self.system_psh(registers::Name::PC)?;
        if entire {
            self.system_psh(registers::Name::U)?;
            self.system_psh(registers::Name::Y)?;
            self.system_psh(registers::Name::X)?;
            self.system_psh(registers::Name::DP)?;
            self.system_psh(registers::Name::B)?;
            self.system_psh(registers::Name::A)?;
        }
        // remember whether we pushed everything onto the stack
        self.reg.cc.set(registers::CCBit::E, entire);
        self.system_psh(registers::Name::CC)?;
        Ok(())
    }
    /// Sets the CC register and stack as appropriate and
    /// then sets PC to the vector for the given interrupt.
    pub fn start_interrupt(&mut self, it: core::InterruptType) -> Result<(), Error> {
        assert!(!self.in_sync);
        // info!("start_interrupt {:?}, vector {:04x}", it, it.vector());
        // if this is an IRQ then we need to push (almost) everything on the stack
        let mut entire = false;
        use crate::core::InterruptType::*;
        let mut if_mask_flags: u8 = 0;
        match it {
            Swi2 | Swi3 => {
                entire = true;
            }
            Irq => {
                entire = true;
                if_mask_flags = 0x10;
            }
            Firq => {
                if_mask_flags = 0x50;
            }
            _ => {
                entire = true;
                if_mask_flags = 0x50;
            }
        }
        // save current state prior to interrupt
        // but only if we aren't already waiting for an interrupt
        // (because if we are, then the state was already saved)
        if !self.in_cwai {
            self.stack_for_interrupt(entire)?;
        }
        // now set the appropriate flags in CC
        self.reg.cc.or_with_byte(if_mask_flags);
        // get the vector for the ISR
        let addr = self._read_u16(AccessType::System, it.vector(), None)?;
        // check to see if the vector points to a zero byte; if so then panic
        let b = self._read_u8(AccessType::System, addr, None)?;
        if b == 0 {
            panic!("interrupt {:?} vector points to zero instruction", it)
        }
        // set the program counter
        self.reg.set_register(registers::Name::PC, u8u16::u16(addr));
        // we're no longer waiting for an interrupt
        self.in_cwai = false;
        Ok(())
    }
    /// Attempt to execute the next instruction at PC.  
    /// If commit=true then commit any/all changes to the machine state.
    /// Otherwise, the changes are only reflected in the instruction::Outcome object.
    /// If list_mode.is_some() then the instruction is not evaluated and Outcome reflects
    /// the state prior to the instruction.
    pub fn exec_next(&mut self, commit: bool) -> Result<instructions::Outcome, Error> {
        let mut start = Instant::now();
        let mut inst = instructions::Instance::new(&self.reg, None);
        let mut op16: u16 = 0; // 16-bit representation of the opcode
        let mut live_ctx: registers::Set = self.reg;

        // get the base op code
        loop {
            inst.buf[inst.size as usize] = self._read_u8(AccessType::Program, live_ctx.pc + inst.size, None)?;
            // inst.buf[inst.size as usize] = unsafe { *self.raw_ram.offset((live_ctx.pc + inst.size) as isize) };
            op16 |= inst.buf[inst.size as usize] as u16;
            inst.size += 1;
            if inst.size == 1 && instructions::is_high_byte_of_16bit_instruction(inst.buf[0]) {
                op16 <<= 8;
                continue;
            }
            break;
        }
        // keep track of how many bytes the opcode takes up
        inst.opsize = inst.size;
        // get the instruction Flavor
        // Note: doing this with if/else rather than ok_or or ok_or_else because it performs better
        inst.flavor = if let Some(flavor) = instructions::opcode_to_flavor(op16) {
            flavor
        } else {
            return Err(runtime_err!(
                Some(self.reg),
                "Bad instruction: {:04X} found at {:04X}",
                op16,
                self.reg.pc
            ));
        };
        self.process_addressing_mode(&mut inst, &mut live_ctx)?;

        assert!(inst.size >= inst.flavor.detail.sz);
        // adjust the program counter before evaluating instructions
        live_ctx.pc = self.checked_pc_add(live_ctx.pc, inst.size, &inst)?;
        let mut o = instructions::Outcome::new(inst, live_ctx);
        // track how long all this preparation took
        self.prep_time += start.elapsed();
        start = Instant::now();

        // evaluate the instruction if we're not in list mode
        if self.list_mode.is_none() {
            (o.inst.flavor.desc.eval)(self, &mut o)?;
        }
        self.eval_time += start.elapsed();
        start = Instant::now();

        // if caller wants to commit the changes and we're not in list mode then commit now
        if commit && self.list_mode.is_none() {
            self.reg = o.new_ctx;
            // and complete any writes to the address space
            if let Some(v) = o.writes.as_ref() {
                for w in v {
                    self._write_u8u16(w.at, w.addr, w.val)?;
                }
            }
        }
        self.commit_time += start.elapsed();

        self.instruction_count += 1;
        self.clock_cycles += o.inst.flavor.detail.clk as u64;
        Ok(o)
    }
    /// Increase the program counter by the given value (rhs).
    /// Returns Error::Runtime in the case of overflow.
    /// Otherwise, Ok.
    #[inline(always)]
    fn checked_pc_add(&self, pc: u16, rhs: u16, inst: &instructions::Instance) -> Result<u16, Error> {
        // avoiding ok_or and ok_or_else to increase performance
        // ok_or would invoke the runtime_err! macro every time (regardless of result)
        // ok_or_else seems to be slightly slower than manually checking with if/else
        if let Some(pc) = pc.checked_add(rhs) {
            Ok(pc)
        } else {
            Err(runtime_err!(
                Some(self.reg),
                "Instruction overflow: instruction {} at {:04X}",
                inst.flavor.desc.name,
                self.reg.pc
            ))
        }
    }

    /// Performs the general setup work for an instruction based on addressing mode.
    /// This includes determining the effective address for the instruction,
    /// updating the instruction size, modifying any registers that are changed by the addressing mode (e.g. ,X+),
    /// and providing a disassembled string representing the operand.
    /// Changes are reflected in the provided inst and live_ctx objects.
    fn process_addressing_mode(
        &self, inst: &mut instructions::Instance, live_ctx: &mut registers::Set,
    ) -> Result<(), Error> {
        match inst.flavor.mode {
            instructions::AddressingMode::Immediate => {
                // effective address is the current PC
                inst.ea = self.checked_pc_add(live_ctx.pc, inst.size, inst)?;
                let addr_size = inst.flavor.detail.sz - inst.size;
                let data = self._read_u8u16(AccessType::Program, inst.ea, addr_size)?;
                inst.size += addr_size;
                if config::help_humans() {
                    inst.operand = Some(match inst.flavor.desc.pbt {
                        instructions::PBT::NA => format!("#${}", data),
                        instructions::PBT::TransferExchange => TEPostByte::to_string(data.u8()),
                        instructions::PBT::PushPull => {
                            PPPostByte::to_string(data.u8(), inst.flavor.desc.reg == registers::Name::U)
                        }
                    });
                }
            }
            instructions::AddressingMode::Direct => {
                // effective address is u16 whose high byte = DP
                // and low byte is stored at the current PC
                inst.ea = ((live_ctx.dp as u16) << 8)
                    | (self._read_u8(
                        AccessType::Program,
                        self.checked_pc_add(live_ctx.pc, inst.size, inst)?,
                        None,
                    )? as u16);
                inst.size += 1;
                if config::help_humans() {
                    inst.operand = Some(format!("${:04X}", inst.ea));
                }
            }
            instructions::AddressingMode::Extended => {
                // effective address is u16 stored at current PC
                inst.ea = self._read_u16(
                    AccessType::Program,
                    self.checked_pc_add(live_ctx.pc, inst.size, inst)?,
                    None,
                )?;
                inst.size += 2;
                if config::help_humans() {
                    inst.operand = Some(format!("${:04X}", inst.ea));
                }
            }
            instructions::AddressingMode::Inherent => {
                // nothing to do. op code itself is sufficient
            }
            instructions::AddressingMode::Relative => {
                let offset_size = inst.flavor.detail.sz - inst.size;
                let offset = self._read_u8u16(
                    AccessType::Program,
                    self.checked_pc_add(live_ctx.pc, inst.size, inst)?,
                    offset_size,
                )?;
                inst.size += offset_size;
                inst.ea = u8u16::u16(self.checked_pc_add(live_ctx.pc, inst.size, inst)?)
                    .signed_offset(offset)
                    .u16();
                if config::help_humans() {
                    inst.operand = Some(format!("{} ({:04x})", offset.i16(), inst.ea));
                }
            }
            instructions::AddressingMode::Indexed => {
                // todo: move this to a function?
                // read the post-byte
                let pb = self._read_u8(
                    AccessType::Program,
                    self.checked_pc_add(live_ctx.pc, inst.size, inst)?,
                    None,
                )?;
                inst.size += 1;
                // is this indirect mode?
                let indirect = (pb & 0b10010000) == 0b10010000;
                // note which register (preg) the register field (rr) is referencing
                let rr = (pb & 0b01100000) >> 5;
                let (ir_ptr, ir_str): (&mut u16, &str) = match rr {
                    0 => (&mut live_ctx.x, "X"),
                    1 => (&mut live_ctx.y, "Y"),
                    2 => (&mut live_ctx.u, "U"),
                    3 => (&mut live_ctx.s, "S"),
                    _ => unreachable!(),
                };
                match pb & 0x8f {
                    0..=0b11111 => {
                        // ,R + 5 bit offset
                        let offset = ((pb & 0b11111) | if pb & 0b10000 != 0 { 0b11100000 } else { 0 }) as i8;
                        let (addr, _) = u16::overflowing_add(*ir_ptr, offset as u16);
                        inst.ea = addr;
                        if config::help_humans() {
                            inst.operand = Some(format!("{},{}", offset, ir_str))
                        }
                    }
                    0b10000000 => {
                        // ,R+
                        if indirect {
                            return Err(Error::new(
                                ErrorKind::Syntax,
                                Some(self.reg),
                                format!("Illegal indirect indexed addressing mode [,R+] at {:04X}", self.reg.pc)
                                    .as_str(),
                            ));
                        }
                        inst.ea = *ir_ptr;
                        let (r, _) = (*ir_ptr).overflowing_add(1);
                        *ir_ptr = r;
                        if config::help_humans() {
                            inst.operand = Some(format!(",{}+", ir_str));
                        }
                    }
                    0b10000001 => {
                        // ,R++
                        inst.ea = *ir_ptr;
                        let (r, _) = (*ir_ptr).overflowing_add(2);
                        *ir_ptr = r;
                        if config::help_humans() {
                            inst.operand = Some(format!(",{}++", ir_str));
                        }
                    }
                    0b10000010 => {
                        // ,-R
                        if indirect {
                            return Err(Error::new(
                                ErrorKind::Syntax,
                                Some(self.reg),
                                format!("Illegal indirect indexed addressing mode [,-R] at {:04X}", self.reg.pc)
                                    .as_str(),
                            ));
                        }
                        let (r, _) = (*ir_ptr).overflowing_sub(1);
                        *ir_ptr = r;
                        inst.ea = *ir_ptr;
                        if config::help_humans() {
                            inst.operand = Some(format!(",-{}", ir_str));
                        }
                    }
                    0b10000011 => {
                        // ,--R
                        let (r, _) = (*ir_ptr).overflowing_sub(2);
                        *ir_ptr = r;
                        inst.ea = *ir_ptr;
                        if config::help_humans() {
                            inst.operand = Some(format!(",--{}", ir_str));
                        }
                    }
                    0b10000100 => {
                        // EA = ,R + 0 offset
                        inst.ea = *ir_ptr;
                        if config::help_humans() {
                            inst.operand = Some(format!(",{}", ir_str));
                        }
                    }
                    0b10000101 => {
                        // EA = ,R + B offset
                        let (addr, _) = u16::overflowing_add(*ir_ptr, (live_ctx.b as i8) as u16);
                        inst.ea = addr;
                        if config::help_humans() {
                            inst.operand = Some(format!("B,{}", ir_str));
                        }
                    }
                    0b10000110 => {
                        // EA = ,R + A offset
                        let (addr, _) = u16::overflowing_add(*ir_ptr, (live_ctx.a as i8) as u16);
                        inst.ea = addr;
                        if config::help_humans() {
                            inst.operand = Some(format!("A,{}", ir_str));
                        }
                    }
                    // 0b10000111 => {} invalid
                    0b10001000 => {
                        // EA = ,R + 8 bit offset
                        let offset = self._read_u8(AccessType::Program, live_ctx.pc + inst.size, None)? as i8;
                        inst.size += 1;
                        let (addr, _) = u16::overflowing_add(*ir_ptr, offset as u16);
                        inst.ea = addr;
                        if config::help_humans() {
                            inst.operand = Some(format!("{},{}", offset, ir_str));
                        }
                    }
                    0b10001001 => {
                        // ,R + 16 bit offset
                        let offset = self._read_u16(AccessType::Program, live_ctx.pc + inst.size, None)? as i16;
                        inst.size += 2;
                        let (addr, _) = u16::overflowing_add(*ir_ptr, offset as u16);
                        inst.ea = addr;
                        if config::help_humans() {
                            inst.operand = Some(format!("{},{}", offset, ir_str));
                        }
                    }
                    // 0b10001010 => {} invalid
                    0b10001011 => {
                        // ,R + D offset
                        let (addr, _) = u16::overflowing_add(*ir_ptr, live_ctx.d);
                        inst.ea = addr;
                        if config::help_humans() {
                            inst.operand = Some(format!("D,{}", ir_str));
                        }
                    }
                    0b10001100 => {
                        // ,PC + 8 bit offset
                        let offset = self._read_u8(AccessType::Program, live_ctx.pc + inst.size, None)? as i8;
                        inst.size += 1;
                        // Note: effective address is relative to the program counter's NEW value (the address of the next instruction)
                        let (pc, _) = u16::overflowing_add(live_ctx.pc, inst.size);
                        let (addr, _) = u16::overflowing_add(pc, offset as u16);
                        inst.ea = addr;
                        if config::help_humans() {
                            inst.operand = Some(format!("{},PC", offset));
                        }
                    }
                    0b10001101 => {
                        // ,PC + 16 bit offset
                        let offset = self._read_u16(AccessType::Program, live_ctx.pc + inst.size, None)? as i16;
                        inst.size += 2;
                        let (pc, _) = u16::overflowing_add(live_ctx.pc, inst.size);
                        let (addr, _) = u16::overflowing_add(pc, offset as u16);
                        inst.ea = addr;
                        if config::help_humans() {
                            inst.operand = Some(format!("{},PC", offset));
                        }
                    }
                    // 0b10001110 => {} invalid
                    0b10001111 => {
                        // EA = [,address]
                        inst.ea = self._read_u16(AccessType::Program, live_ctx.pc + inst.size, None)?;
                        if config::help_humans() {
                            inst.operand = Some(format!("[{:04X}]", inst.ea));
                        }
                        inst.size += 2;
                    }
                    _ => {
                        return Err(Error::new(
                            ErrorKind::Syntax,
                            Some(self.reg),
                            format!(
                                "Invalid indexed addressing post-byte {:02X} in instruction at {:04X}",
                                pb, self.reg.pc
                            )
                            .as_str(),
                        ));
                    }
                }
                // if indirect flag is set then set inst.ea to self.ram[inst.ea]
                if indirect {
                    inst.ea = self._read_u16(AccessType::Generic, inst.ea, None)?;
                }
            }
            _ => panic!("Invalid addressing mode! {:?}", inst.flavor.mode),
        }
        Ok(())
    }
}
