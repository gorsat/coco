//! # A TRS-80 Color Computer simulator
//!
//! ## Options
//! Help for command line options is available using -h or --help.
#[macro_use]
mod macros;
#[macro_use]
mod term;
mod acia;
mod assembler;
#[cfg(test)]
mod audio_test;
mod config;
mod core;
mod debug;
mod devmgr;
mod error;
mod hex;
mod instructions;
mod memory;
mod obj;
mod parse;
mod pia;
mod program;
mod registers;
mod runtime;
mod sam;
mod sound;
mod test;
mod u8oru16;
mod vdg;
use crate::assembler::Assembler;
use std::collections::{HashMap, VecDeque};
use std::ffi::OsStr;
use std::path::Path;
use std::result::Result;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::*;
use std::sync::Arc;
use std::time::Instant;
use std::{fmt, io, thread};
pub(crate) use u8oru16::u8u16;
pub(crate) use {crate::core::Core, devmgr::*, error::*, program::*};

fn main() {
    config::init();
    term::init();
    // The device manager has to live on the main thread
    // because it opens a window via minifb (must be done on main thread on some OS's)
    // but SAM, PIA and VDG are all accessed from another thread (the "core" thread)
    // Ideally, this would be the other way around (main thread == core thread and window on another thread).
    let mut dm = DeviceManager::new();
    // Get threadsafe clones of peripherals for use on the "core" thread.
    let ram = dm.get_ram();
    let vdg = dm.get_vdg();
    let pia0 = dm.get_pia0();
    let pia1 = dm.get_pia1();
    let sam = dm.get_sam();
    let simulation_complete = Arc::new(AtomicBool::new(false));
    let complete = simulation_complete.clone();
    // the simulated computer runs on a separate thread (aka "core" thread)
    thread::spawn(move || {
        let acia_addr = if !config::ARGS.acia_enable {
            None
        } else {
            Some(config::ARGS.acia_addr)
        };
        //  create a CPU simulator
        let mut core = Core::new(ram, sam, vdg, pia0, pia1, config::ARGS.ram_top, acia_addr);
        if let Err(e) = compute_thread(&mut core) {
            println!("SIMULATOR ERROR: {}", e);
        }
        complete.store(true, Release);
    });
    while dm.is_running() && !simulation_complete.load(Acquire) {
        dm.update();
    }
}
/// The emulator's CPU runs on this thread.
/// Load up everything the user has requested and then start the CPU running.
/// The load order is as follows:
/// - load the cartridge if one is specified on the command line
/// - load any ROM(s) specified in the config file
/// - load any code (asm or hex) specified in the config file
/// - load code specified on the command line
/// 
/// This load order allows the user to replace segments of the code in
/// ROM or cartridge programs with their own custom code.
fn compute_thread(core: &mut Core) -> Result<(), Error> {
    // try to load a cartridge
    if let Some(cart) = config::ARGS.cart.as_ref() {
        core.load_cart(cart)?;
    }
    // try to load contents of ROM
    if let Some(c) = config::ARGS.config_file.as_ref() {
        if let Some(roms) = &c.load_rom {
            for r in roms {
                info!("loading ROM at {:04x} from: {}", r.addr, r.path.display());
                core.load_bin(&r.path, r.addr)?;
            }
        } else {
            warn!("No ROMs specified in config file.");
        }
        if let Some(code) = &c.load_code {
            for h in code {
                info!("loading code from: {}", h.path.display());
                core.load_program_from_file(&h.path)?;
            }
        } else {
            info!("No code specified in config file.");
        }
    }
    // try to load other code provided by user
    if let Some(path) = config::ARGS.load.as_ref() {
        // load program
        info!("Loading {}", path.display());
        core.load_program_from_file(path)?;
    }
    info!("Press <ctrl-c> to exit.");
    // put the simulator in a clean reset state and start running
    core.reset()?;
    core.exec()?;

    Ok(())
}
