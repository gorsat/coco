use crate::pia::*;
use crate::sam::*;
use crate::sound;
use crate::vdg::*;

use std::sync::RwLock;
use std::sync::{Arc, Mutex};

use minifb::{Scale, ScaleMode, Window, WindowOptions};

// DeviceManager should be instantiated on the main thread and then clones of its
// member fields can be sent to other threads. DeviceManger methods must only be
// called on the main thread.
pub struct DeviceManager {
    window: minifb::Window,
    display: Vec<u32>,
    _audio: sound::AudioDevice,
    ram: Arc<RwLock<Vec<u8>>>,
    sam: Arc<Mutex<Sam>>,
    vdg: Arc<Mutex<Vdg>>,
    pia0: Arc<Mutex<Pia0>>,
    pia1: Arc<Mutex<Pia1>>,
}
impl DeviceManager {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        // todo: ram no longer needs to be wrapped in Arc or RwLock
        let ram = Arc::new(RwLock::new(vec![0u8; 0x10000]));
        Self::with_ram(ram, 0)
    }
    pub fn with_ram(ram: Arc<RwLock<Vec<u8>>>, vram_offset: usize) -> Self {
        // Initialize the screen (window)
        let mut window = Window::new(
            "Rusty CoCo",
            SCREEN_DIM_X,
            SCREEN_DIM_Y,
            WindowOptions {
                resize: true,
                scale_mode: ScaleMode::AspectRatioStretch,
                scale: Scale::X4,
                ..WindowOptions::default()
            },
        )
        .expect("Failed to open window");
        window.limit_update_rate(Some(SCREEN_REFRESH_PERIOD));
        // Initialize audio device
        // todo: the AudioDevice should probably live in pia1
        let mut _audio = sound::AudioDevice::try_new().expect("failed to create audio device");
        // Arc<(Mutex<bool>, Condvar)>
        let vdg = Arc::new(Mutex::new(Vdg::with_ram(ram.clone(), vram_offset)));
        // Pia1 needs to communicate directly with the audio output device (which it does via AudioRingBuffer)
        let pia1 = Arc::new(Mutex::new(Pia1::new(_audio.take_sender())));
        DeviceManager {
            window,
            display: vec![Color::Green.to_rgb(); SCREEN_DIM_X * SCREEN_DIM_Y],
            _audio,
            ram,
            sam: Arc::new(Mutex::new(Sam::new())),
            vdg,
            pia0: Arc::new(Mutex::new(Pia0::new(pia1.clone()))),
            pia1,
        }
    }

    pub fn get_vdg(&self) -> Arc<Mutex<Vdg>> { self.vdg.clone() }
    pub fn get_pia0(&self) -> Arc<Mutex<Pia0>> { self.pia0.clone() }
    pub fn get_pia1(&self) -> Arc<Mutex<Pia1>> { self.pia1.clone() }
    pub fn get_ram(&self) -> Arc<RwLock<Vec<u8>>> { self.ram.clone() }
    pub fn get_sam(&self) -> Arc<Mutex<Sam>> { self.sam.clone() }
    pub fn is_running(&self) -> bool { self.window.is_open() }
    pub fn update(&mut self) {
        let mut redraw = false;
        {
            // pia0 handles keyboard input
            let mut pia0 = self.pia0.lock().unwrap();
            pia0.update(&self.window);
        }
        let mode;
        let css;
        let vram_offset;
        {
            // use SAM and PIA1 to determine current VDG mode
            let sam = self.sam.lock().unwrap();
            let pia1 = self.pia1.lock().unwrap();
            let pia_bits = pia1.get_vdg_bits();
            mode = VdgMode::try_from_pia_and_sam(pia_bits, sam.get_vdg_bits());
            css = pia_bits & 1 == 1;
            // get the starting address of VRAM from the SAM
            vram_offset = sam.get_vram_start() as usize;
        }
        // only try rendering the screen if we have a valid VdgMode
        if let Some(mode) = mode {
            let mut vdg = self.vdg.lock().unwrap();
            vdg.set_mode(mode);
            vdg.set_vram_offset(vram_offset);
            // convert contents of VRAM to pixels for display
            redraw = vdg.render(&mut self.display, css);
        }
        if redraw {
            self.window
                .update_with_buffer(&self.display, SCREEN_DIM_X, SCREEN_DIM_Y)
                .expect("minifb update_with_buffer failed");
        } else {
            self.window.update();
        }
    }
}
