use std::time::{Duration, Instant};

use super::*;
use sound::*;

const USE_DATA: bool = false;
const DATA: &[f32] = &[0.0, 0.4, 0.0, -0.4];
// const DATA: &[f32] = &[0.2, -0.2, 0.2, -0.2];
#[test]
fn basic_audio() -> Result<(), Error> {
    let mut a = AudioDevice::try_new()?;
    let samples_per_cycle = if USE_DATA { DATA.len() } else { 8usize };
    let time_slice = Duration::from_secs_f32(1.0 / (440.0 * samples_per_cycle as f32));
    info!("audio test data sample period = {} usec", time_slice.as_micros());
    let sender = a.take_sender();
    let start = Instant::now();
    let amplitude = 0.4f32;
    let mut i = 0usize;
    thread::sleep(Duration::from_millis(100));
    while start.elapsed() < Duration::from_millis(200) {
        let data = if USE_DATA {
            DATA[i]
        } else {
            ((i as f32 / 4.0) * std::f32::consts::PI).sin() * amplitude
        };
        let time = Instant::now();
        sender
            .send(AudioSample { data, time })
            .expect("failed to send audio data on channel");
        i = (i + 1) % samples_per_cycle;
        while Instant::now() - time < time_slice {/* spin */}
        // let send_time = Instant::now() - time;
        // if send_time < time_slice {
        //     spin_sleep::sleep(time_slice - send_time);
        // }
        // assert!(Instant::now() - time < time_slice * 2);
    }
    sender
        .send(AudioSample {
            data: 0.0,
            time: Instant::now(),
        })
        .unwrap();
    spin_sleep::sleep(Duration::from_millis(210));
    Ok(())
}
