#![allow(unused)]
use std::path::PathBuf;

use clap::Parser;
use clap_num::maybe_hex;
use lazy_static::lazy_static;
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(author,version,about,long_about=None)]
pub struct Args {
    /// Assembly (.asm, .s) or Hex (.hex) file to assemble/run/debug
    #[arg(long)]
    pub load: Option<PathBuf>,

    /// Enable ACIA emulation
    #[arg(long)]
    pub acia_enable: bool,

    /// Address at which to map the ACIA (hex ok with '0x')
    #[arg(long,value_parser=maybe_hex::<u16>, default_value_t=0xffd0_u16)]
    pub acia_addr: u16,

    /// TCP port on which to expose ACIA
    #[arg(long, default_value_t = 6809_u16)]
    pub acia_port: u16,

    /// Print ACIA debug information
    #[arg(long)]
    pub acia_debug: bool,

    /// Swap the case of alpha ASCII characters received via ACIA (a->A;A->a)
    #[arg(long)]
    pub acia_case: bool,

    /// Break into the debugger before running the program (only if debugger enabled)
    #[arg(short, long)]
    pub break_start: bool,

    /// Remove blank and comment-only lines from program listing
    #[arg(long)]
    pub code_only: bool,

    /// Load a cartridge from file
    #[arg(long)]
    pub cart: Option<PathBuf>,

    /// Run with debugger enabled
    #[arg(short, long)]
    pub debug: bool,

    /// The number of instructions to keep in the execution history when debugging
    #[arg(long, default_value_t = 100)]
    pub history: usize,

    /// If there is a program listing then dump it to stdout
    #[arg(short, long)]
    pub list: bool,

    /// Disable automatic branch->long_branch conversion
    #[arg(long)]
    pub lbr_disable: bool,

    /// Limits the clock speed in MHz (default is unlimited)
    #[arg(short, long)]
    pub mhz: Option<f32>,

    /// No automatic loading of symbols
    #[arg(short, long)]
    pub no_auto_sym: bool,

    /// Automatically evaluate expressions using PEMDAS rather than left-to-right
    #[arg(short, long)]
    pub pemdas: bool,

    /// Display perf data (only interesting for longer-running programs)
    #[arg(long)]
    pub perf: bool,

    /// Set the top RAM address
    #[arg(long,value_parser=maybe_hex::<u16>, default_value_t=0x7fff_u16)]
    pub ram_top: u16,

    /// Override the reset vector
    #[arg(long,value_parser=maybe_hex::<u16>)]
    pub reset_vector: Option<u16>,

    /// Set the duration in seconds for which the program should run
    #[arg(short, long)]
    pub time: Option<f32>,

    /// Trace each machine instruction as it is executed
    #[arg(long)]
    pub trace: bool,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Write output files after assembly (.lst, .sym, .hex)
    #[arg(short, long)]
    pub write_files: bool,

    /// Path to toml config file
    #[arg(long, default_value_os_t=PathBuf::from("./coco.yaml"))]
    pub config_file_path: PathBuf,

    /// Config loaded from file
    #[arg(skip)]
    pub config_file: Option<ConfigFile>,
}

#[derive(Debug, Deserialize)]
pub struct RomSpec {
    pub path: PathBuf,
    pub addr: u16,
}
#[derive(Debug, Deserialize)]
pub struct ConfigFile {
    // files containing binary data to load into ROM
    pub load_rom: Option<Vec<RomSpec>>,
    pub load_code: Option<Vec<LoadCode>>,
}
#[derive(Debug, Deserialize)]
pub struct LoadCode {
    pub path: PathBuf,
}
lazy_static! {
    pub static ref ARGS: Args = if cfg!(test) {
        // manually set parameters for running tests
        Args::parse_from(["test", "test", "--run"])
    } else {
        let mut args = Args::parse();
        let s = std::fs::read_to_string(&args.config_file_path)
            .unwrap_or_else(|_| {
                warn!("Failed to open config file \"{}\"", &args.config_file_path.display());
                String::default()
            });
        args.config_file = Some(serde_yaml::from_str(&s).unwrap());
        args
    };
}

pub fn init() {}
pub fn auto_load_syms() -> bool { !ARGS.no_auto_sym && ARGS.debug }
pub fn debug() -> bool { ARGS.debug }
pub fn help_humans() -> bool { ARGS.debug || ARGS.trace }
