use crate::error::*;
use cpal::traits::*;
use std::{
    collections::VecDeque,
    sync::{mpsc, Arc, Mutex},
    thread,
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy)]
pub struct AudioSample {
    pub data: f32,
    pub time: Instant,
}

impl AudioSample {
    pub fn new(data: f32) -> Self {
        AudioSample {
            data,
            time: Instant::now(),
        }
    }
}

#[allow(dead_code)]
pub struct AudioDevice {
    device: cpal::Device,
    stream: cpal::Stream,
    sndr: Option<mpsc::Sender<AudioSample>>,
    thread: JoinHandle<()>,
    buffering: bool,
    channels: usize,
    sample_rate: usize,
    buffer_frames: usize,
}
impl AudioDevice {
    pub fn try_new() -> Result<Self, Error> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(general_err!("failed to open audio output device"))?;
        info!(
            "using audio output device: {}",
            device.name().unwrap_or("<unknown>".to_string())
        );
        let dc = device
            .default_output_config()
            .map_err(|e| general_err!("no default audio config: {e}"))?;
        let channels = (dc.channels() as usize).min(2);
        let sample_rate = dc.sample_rate().0 as usize;
        let buffer_frames = match *dc.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => max.min(2048).max(min) as usize,
            _ => panic!(),
        };
        info!(
            "audio output stream config: channels={channels}, sample_rate={sample_rate}, buffer_frames={buffer_frames}"
        );
        let config = cpal::StreamConfig {
            channels: channels as u16,
            sample_rate: cpal::SampleRate(sample_rate as u32),
            buffer_size: cpal::BufferSize::Fixed(buffer_frames as u32),
        };
        let (sndr, rcvr) = mpsc::channel();
        let mut pipeline = AudioPipeline::new(rcvr, sample_rate, buffer_frames);
        let bp = Arc::new(Mutex::new(SourceBufferPool::new(buffer_frames)));
        let bpc = bp.clone();
        let mut streaming = false;
        let mut buf_opt: Option<SampleQue<f32>> = None;
        // Note: Assuming here that most audio devices support f32 samples!
        let stream = device
            .build_output_stream(
                &config,
                move |mut output: &mut [f32], _| {
                    let mut sample_num = 0;
                    loop {
                        if buf_opt.is_none() {
                            // we don't have a source data buffer yet
                            // if we're already streaming or if there are multiple full source data buffers
                            // then try to get a source data buffer to copy to the output buffer
                            let mut bpc = bpc.lock().unwrap();
                            if streaming || bpc.full_buffer_count() > 1 {
                                buf_opt = bpc.get_full_buffer();
                            }
                        }
                        if buf_opt.is_none() {
                            // failed to get a source data buffer
                            // remember that we stopped streaming
                            streaming = false;
                            // fill the rest of the output buffer with zero and return
                            output.fill_with_sample(sample_num, channels, 0.0);
                            return;
                        }
                        let mut buf = buf_opt.take().unwrap();
                        streaming = true;
                        loop {
                            if output.samples_remaining(sample_num, channels) == 0 {
                                // we're done filling the output buffer
                                // save the current source buffer for next time
                                buf_opt.replace(buf);
                                return;
                            }
                            if let Some(sample_data) = buf.read_next_sample() {
                                output.write_sample(sample_num, channels, sample_data);
                                sample_num += 1;
                            } else {
                                // we ran out of source data; need to try to get another buffer
                                let mut bpc = bpc.lock().unwrap();
                                bpc.put_empty_buffer(buf);
                                break;
                            }
                        }
                    }
                },
                move |e| warn!("audio stream error: {}", e),
                None, // None=blocking, Some(Duration)=timeout
            )
            .map_err(|e| general_err!("failed to build audio output stream: {}", e))?;
        stream
            .play()
            .map_err(|e| general_err!("failed to start audio output stream: {}", e))?;
        let thread = thread::spawn(move || pipeline.thread(bp));
        Ok(AudioDevice {
            device,
            stream,
            sndr: Some(sndr),
            thread,
            buffering: false,
            channels,
            sample_rate,
            buffer_frames,
        })
    }
    pub fn take_sender(&mut self) -> mpsc::Sender<AudioSample> { self.sndr.take().expect("sender already taken!") }
}
/// AudioPipeline is really just a container for some state used by the pipeline thread.
/// This thread converts aperiodic DAC changes into a stream of periodic samples that can
/// then be written directly to the audio device.
/// The thread provides some buffering between DAC writes and the ultimate sound output
/// which significantly reduces glitches in a cooperative multitasking environment.
struct AudioPipeline {
    rcvr: mpsc::Receiver<AudioSample>,
    last_written: AudioSample,
    wrote_last_cycle: bool,
    sample_duration: Duration,
    buffer_duration: Duration,
    silent_buffer: bool,
    wrote_sound: bool,
    gain: f32,
    avg_window: AvgWindow<f32>,
}
impl AudioPipeline {
    fn new(rcvr: mpsc::Receiver<AudioSample>, sample_rate: usize, buffer_frames: usize) -> Self {
        let sample_duration = Duration::from_secs_f32(1.0 / (sample_rate as f32));
        info!("pipeline sample period = {} usec", sample_duration.as_micros());
        AudioPipeline {
            rcvr,
            last_written: AudioSample::new(0.0),
            wrote_last_cycle: false,
            sample_duration,
            buffer_duration: buffer_frames as u32 * sample_duration,
            silent_buffer: true,
            wrote_sound: false,
            gain: 0.95,
            avg_window: AvgWindow::<f32>::new(2),
        }
    }
    fn thread(&mut self, bp: Arc<Mutex<SourceBufferPool>>) {
        let mut buffer_opt: Option<SampleQue<f32>> = None;
        let mut buffer_index: usize = 0;
        let mut pending_sample: Option<AudioSample> = None;
        let mut loop_time = Instant::now();
        let mut last_rcv_time = Instant::now();
        loop {
            let sample = if let Some(sample) = pending_sample.take() {
                // we already have a sample that we couldn't write
                // sleep because we're writing faster than the audio device is consuming
                spin_sleep::sleep(self.sample_duration);
                sample
            } else {
                // try to get a new sample
                match self.rcvr.try_recv() {
                    Ok(sample) => {
                        last_rcv_time = Instant::now();
                        sample
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        // no sample ready
                        if Instant::now() - last_rcv_time >= self.buffer_duration {
                            // it's been a while since we received any sound data
                            // so reset our cache of the previous sample
                            self.last_written = AudioSample::new(0.0);
                            // also reset our averaging window
                            self.avg_window.clear();
                        }
                        if (buffer_opt.is_some() || self.last_written.data != 0.0)
                            && (Instant::now() - loop_time > self.sample_duration)
                        {
                            // if we've got a buffer already or we're writing non-zero data
                            // and if enough time has passed then reuse the last sample we sent
                            AudioSample {
                                data: self.last_written.data,
                                time: self.last_written.time + self.sample_duration,
                            }
                        } else {
                            // wait and then check again for a new sample
                            spin_sleep::sleep(self.sample_duration);
                            continue;
                        }
                    }
                    _ => {
                        // the channel is gone; end the thread
                        break;
                    }
                }
            };
            loop_time = Instant::now();
            // we have a sample; now we do something with it

            // make sure we have a buffer to write into
            'get_buffer: loop {
                if buffer_opt.is_none() {
                    let mut bp = bp.lock().unwrap();
                    buffer_opt = bp.get_empty_buffer();
                    buffer_index = 0;
                    self.silent_buffer = true;
                }
                if let Some(mut buffer) = buffer_opt.take() {
                    // we have a buffer; see if we need to fill in any time prior to the current sample
                    let elapsed = sample.time - self.last_written.time;
                    // if there is a gap between the new sample and the previous sample
                    // then fill the gap with linear interpolations between the two
                    if elapsed > self.sample_duration && elapsed < self.buffer_duration {
                        let (index, _) = self.interpolate_fill(sample, &mut buffer, buffer_index);
                        buffer_index = index;
                    }
                    // now write the new sample into the buffer
                    if 0 == self.write_sample(sample, &mut buffer, buffer_index) {
                        // the buffer is full; return it to the buffer pool
                        if self.silent_buffer {
                            // the buffer is just full of silence; recycle it
                            bp.lock().unwrap().put_empty_buffer(buffer);
                        } else {
                            // the buffer has meaningful data
                            bp.lock().unwrap().put_full_buffer(buffer);
                        }
                        // and try to get a new buffer
                        continue 'get_buffer;
                    }
                    // we successfully wrote the new sample into the buffer
                    buffer_index += 1;
                    //
                    buffer_opt.replace(buffer);
                    break 'get_buffer;
                } else {
                    pending_sample = Some(sample);
                    spin_sleep::sleep(self.sample_duration);
                }
            }
        }
    }

    /// This is the only place where samples are written into pipeline buffers.
    #[inline(always)]
    fn write_sample(&mut self, mut sample: AudioSample, buf: &mut SampleQue<f32>, sample_index: usize) -> usize {
        if buf.capacity_remaining() == 0 {
            return 0;
        }
        assert!(sample_index == buf.len());
        // apply gain
        sample.data *= self.gain;
        // apply some simple limiting
        sample.data = sample.data.min(0.95);
        sample.data = sample.data.max(-0.95);
        // apply some smoothing (low-pass filter)
        self.avg_window.push(sample.data);
        sample.data = self.avg_window.avg();
        // finally write the sample to the buffer
        buf.write_next_sample(sample.data);
        // update state based on what we wrote
        self.last_written = sample;
        self.wrote_last_cycle = true;
        if sample.data != 0.0 {
            self.silent_buffer = false;
            self.wrote_sound = true;
        }
        1
    }
    /// interpolate_fill uses simple linear interpolation to fill gaps between audio samples.
    #[inline(always)]
    fn interpolate_fill(
        &mut self, end_sample: AudioSample, out: &mut SampleQue<f32>, sample_index: usize,
    ) -> (usize, Duration) {
        let start_sample = self.last_written;
        let mut sample = start_sample;
        let mut index = sample_index;
        let mut elapsed = Duration::ZERO;
        let start_time = start_sample.time + self.sample_duration;
        if end_sample.time > start_time {
            let mut period = end_sample.time - start_time;
            if period > self.buffer_duration {
                period = self.buffer_duration;
                sample.time = end_sample.time.checked_sub(period).unwrap();
            }
            let mut sample_count = (period.as_secs_f32() / self.sample_duration.as_secs_f32())
                .round()
                .max(1.0) as usize;
            let delta = (end_sample.data - start_sample.data) / sample_count as f32;
            while sample_count > 0 {
                sample_count -= 1;
                sample.time += self.sample_duration;
                sample.data += delta;
                if self.write_sample(sample, out, index) == 0 {
                    // ran out of space in the buffer
                    break;
                }
                index += 1;
                elapsed += self.sample_duration;
            }
        }
        (index, elapsed)
    }
}
/// Manages a set of buffers used to move data between the pipeline thread and the
/// audio device's output thread.
pub struct SourceBufferPool {
    empty: Vec<SampleQue<f32>>,
    full: VecDeque<SampleQue<f32>>,
}
impl SourceBufferPool {
    fn get_full_buffer(&mut self) -> Option<SampleQue<f32>> { self.full.pop_front() }
    fn put_full_buffer(&mut self, buffer: SampleQue<f32>) { self.full.push_back(buffer); }
    fn full_buffer_count(&self) -> usize { self.full.len() }
    fn get_empty_buffer(&mut self) -> Option<SampleQue<f32>> { self.empty.pop() }
    fn put_empty_buffer(&mut self, mut buffer: SampleQue<f32>) {
        buffer.clear();
        self.empty.push(buffer);
    }
    fn new(buffer_frames: usize) -> Self {
        Self {
            // Reasoning for 4 buffers - We want to have enough buffers such that we could simultaneously have
            // buffers in each of the following states: reading, writing, full, empty
            empty: vec![
                SampleQue::new(buffer_frames),
                SampleQue::new(buffer_frames),
                SampleQue::new(buffer_frames),
                SampleQue::new(buffer_frames),
            ],
            full: Default::default(),
        }
    }
}

/// This trait provides the API for buffers used between the pipeline and output threads.
/// It's a trait because the underlying implementation used to be a different type.
trait SourceSampleBuffer<T> {
    fn read_next_sample(&mut self) -> Option<T>;
    fn write_next_sample(&mut self, sample_data: T) -> bool;
    fn capacity_remaining(&self) -> usize;
    fn capacity(&self) -> usize;
    fn len(&self) -> usize;
    fn clear(&mut self);
}

impl<T> SourceSampleBuffer<T> for SampleQue<T>
where
    T: Copy,
{
    fn read_next_sample(&mut self) -> Option<T> {
        if self.head == self.tail {
            None
        } else {
            let i = self.head;
            self.head += 1;
            Some(self.q[i])
        }
    }
    fn write_next_sample(&mut self, sample_data: T) -> bool {
        if self.tail == self.q.len() {
            false
        } else {
            self.q[self.tail] = sample_data;
            self.tail += 1;
            true
        }
    }
    fn capacity_remaining(&self) -> usize { self.q.len() - self.tail }
    fn capacity(&self) -> usize { self.q.len() }
    fn len(&self) -> usize { self.tail - self.head }
    fn clear(&mut self) {
        self.head = 0;
        self.tail = 0;
    }
}
/// This is the type of buffers used to pass data between the pipeline and output threads
#[derive(Debug)]
struct SampleQue<T> {
    q: Box<[T]>,
    head: usize,
    tail: usize,
}
impl<T> SampleQue<T>
where
    T: Clone + Default,
{
    fn new(buffer_samples: usize) -> Self {
        Self {
            q: vec![T::default(); buffer_samples].into_boxed_slice(),
            head: 0,
            tail: 0,
        }
    }
}
/// A trait to wrap the output buffer (a slice of T) with some helpful methods
trait OutputSampleBuffer<T> {
    fn write_sample(&mut self, sample_num: usize, channels: usize, sample_data: T);
    fn fill_with_sample(&mut self, sample_num: usize, channels: usize, sample_data: T);
    fn samples_remaining(&self, sample_num: usize, channels: usize) -> usize;
}
impl<T> OutputSampleBuffer<T> for &mut [T]
where
    T: Copy,
{
    #[inline(always)]
    fn write_sample(&mut self, sample_num: usize, channels: usize, sample_data: T) {
        assert!(self.samples_remaining(sample_num, channels) > 0);
        self[sample_num * channels..(sample_num + 1) * channels]
            .iter_mut()
            .for_each(|p| *p = sample_data)
    }
    #[inline(always)]
    fn samples_remaining(&self, sample_num: usize, channels: usize) -> usize {
        if self.len() / channels < sample_num {
            0
        } else {
            (self.len() / channels) - sample_num
        }
    }
    #[inline(always)]
    fn fill_with_sample(&mut self, start_sample_num: usize, channels: usize, fill_sample_data: T) {
        for i in start_sample_num..self.samples_remaining(start_sample_num, channels) {
            self.write_sample(i, channels, fill_sample_data)
        }
    }
}
/// A simple rolling average window that defaults unused entries to zero
struct AvgWindow<T> {
    ring: Vec<T>,
    size: usize,
    head: usize,
    tail: usize,
}
impl<T> AvgWindow<T>
where
    T: Copy + Default + std::ops::Add<Output = T> + std::ops::Div<Output = T> + std::convert::From<u16>,
{
    fn new(size: usize) -> Self {
        Self {
            ring: vec![0.into(); size],
            size,
            head: 0,
            tail: 0,
        }
    }
    fn clear(&mut self) {
        self.head = 0;
        self.tail = self.size - 1;
        self.ring.iter_mut().for_each(|t| *t = 0.into())
    }
    fn push(&mut self, t: T) {
        self.tail = (self.tail + 1) % self.size;
        self.head = (self.tail + 1) % self.size;
        self.ring[self.tail] = t;
    }
    fn avg(&self) -> T {
        let mut sum: T = 0.into();
        (0..self.size).for_each(|i| sum = sum + self.ring[(i + self.head) % self.size]);
        sum.div(((self.size & 0xffff) as u16).into())
    }
}
