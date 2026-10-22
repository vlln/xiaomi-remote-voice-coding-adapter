//! Cross-platform audio output.
//!
//! The BLE task pushes processed 16 kHz mono PCM into a lock-free ring buffer;
//! a dedicated thread owns the `cpal` stream and converts it to 48 kHz stereo
//! for a virtual audio device (BlackHole / VB-CABLE / PipeWire null sink).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SampleRate, SizedSample, StreamConfig};
use parking_lot::Mutex;
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use serde::Serialize;
use tracing::{error, info, warn};

use crate::core::AudioSink;
use crate::core::dsp::Resampler;

const OUTPUT_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const RING_SECONDS: usize = 5;

#[derive(Debug, Clone, Serialize)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no audio output devices found")]
    NoDevices,
    #[error("audio device not found: {0}")]
    DeviceNotFound(String),
    #[error("audio device does not support 48 kHz stereo: {0}")]
    UnsupportedConfig(String),
    #[error("failed to build audio stream: {0}")]
    Build(String),
    #[error("audio output thread did not start: {0}")]
    Thread(String),
}

pub fn default_device_hint() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "BlackHole 2ch"
    }
    #[cfg(target_os = "windows")]
    {
        "CABLE Input"
    }
    #[cfg(target_os = "linux")]
    {
        "Xiaomi Remote"
    }
}

pub fn list_output_devices() -> Vec<AudioDeviceInfo> {
    let host = cpal::default_host();
    let default_name = host.default_output_device().and_then(|d| d.name().ok());
    let mut devices: Vec<AudioDeviceInfo> = host
        .output_devices()
        .map(|iter| {
            iter.filter_map(|device| device.name().ok())
                .map(|name| AudioDeviceInfo {
                    is_default: default_name.as_deref() == Some(name.as_str()),
                    name,
                })
                .collect()
        })
        .unwrap_or_default();
    devices.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    devices.dedup_by(|a, b| a.name == b.name);
    devices
}

pub fn device_exists(name: &str) -> bool {
    list_output_devices()
        .iter()
        .any(|d| d.name.eq_ignore_ascii_case(name))
}

enum Control {
    Stop,
}

pub struct CpalSink {
    producer: Mutex<HeapProd<f32>>,
    active: Arc<AtomicBool>,
    draining: Arc<AtomicBool>,
    reset: Arc<AtomicBool>,
    control: Mutex<Option<Sender<Control>>>,
    thread: Mutex<Option<JoinHandle<()>>>,
    resampler: Mutex<Resampler>,
    scratch: Mutex<Vec<f32>>,
    dropped: AtomicU64,
    device_name: String,
}

impl CpalSink {
    pub fn new(device_name: &str) -> Result<Self, AudioError> {
        let capacity = OUTPUT_RATE as usize * CHANNELS as usize * RING_SECONDS;
        let ring = HeapRb::<f32>::new(capacity);
        let (producer, consumer) = ring.split();

        let active = Arc::new(AtomicBool::new(false));
        let draining = Arc::new(AtomicBool::new(false));
        let reset = Arc::new(AtomicBool::new(false));
        let (control_tx, control_rx) = mpsc::channel::<Control>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

        let thread_active = active.clone();
        let thread_draining = draining.clone();
        let thread_reset = reset.clone();
        let name = device_name.to_string();
        let handle = thread::Builder::new()
            .name("xiaomi-audio-output".into())
            .spawn(move || {
                let stream = build_stream(
                    &name,
                    consumer,
                    thread_active,
                    thread_draining,
                    thread_reset,
                );
                match stream {
                    Ok(stream) => {
                        if let Err(err) = stream.play() {
                            let _ = ready_tx.send(Err(err.to_string()));
                            return;
                        }
                        info!(device = %name, "audio output stream ready");
                        let _ = ready_tx.send(Ok(()));
                        let _ = control_rx.recv();
                        drop(stream);
                        info!("audio output stream stopped");
                    }
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                    }
                }
            })
            .map_err(|e| AudioError::Thread(e.to_string()))?;

        match ready_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Self {
                producer: Mutex::new(producer),
                active,
                draining,
                reset,
                control: Mutex::new(Some(control_tx)),
                thread: Mutex::new(Some(handle)),
                resampler: Mutex::new(Resampler::new()),
                scratch: Mutex::new(Vec::with_capacity(1024)),
                dropped: AtomicU64::new(0),
                device_name: device_name.to_string(),
            }),
            Ok(Err(err)) => {
                let _ = handle.join();
                Err(AudioError::Build(err))
            }
            Err(err) => {
                let _ = control_tx.send(Control::Stop);
                Err(AudioError::Thread(format!("timeout: {err}")))
            }
        }
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn dropped_samples(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl AudioSink for CpalSink {
    fn start(&self) {
        *self.resampler.lock() = Resampler::new();
        self.reset.store(true, Ordering::Relaxed);
        self.draining.store(false, Ordering::Relaxed);
        self.active.store(true, Ordering::Relaxed);
    }

    fn write(&self, samples: &[i16]) {
        if !self.active.load(Ordering::Relaxed) || samples.is_empty() {
            return;
        }
        let mono = self.resampler.lock().resample(samples);
        let mut scratch = self.scratch.lock();
        scratch.clear();
        for sample in mono {
            let value = f32::from(sample) / 32768.0;
            scratch.push(value);
            scratch.push(value);
        }
        let written = self.producer.lock().push_slice(&scratch);
        if written < scratch.len() {
            let dropped = (scratch.len() - written) as u64;
            self.dropped.fetch_add(dropped, Ordering::Relaxed);
            warn!(dropped, "audio ring buffer full; dropped samples");
        }
    }

    fn stop(&self, drain: bool) {
        if drain {
            self.draining.store(true, Ordering::Relaxed);
        } else {
            self.active.store(false, Ordering::Relaxed);
            self.draining.store(false, Ordering::Relaxed);
            self.reset.store(true, Ordering::Relaxed);
        }
    }
}

impl Drop for CpalSink {
    fn drop(&mut self) {
        if let Some(tx) = self.control.lock().take() {
            let _ = tx.send(Control::Stop);
        }
        if let Some(handle) = self.thread.lock().take() {
            let _ = handle.join();
        }
    }
}

fn build_stream(
    device_name: &str,
    consumer: HeapCons<f32>,
    active: Arc<AtomicBool>,
    draining: Arc<AtomicBool>,
    reset: Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = resolve_device(&host, device_name).map_err(|e| e.to_string())?;
    let supported = pick_config(&device).map_err(|e| e.to_string())?;
    let sample_format = supported.sample_format();
    let mut config: StreamConfig = supported.config();
    config.buffer_size = cpal::BufferSize::Fixed(480);
    let err_fn = |err| error!(%err, "audio output stream error");

    match sample_format {
        SampleFormat::F32 => {
            build_typed::<f32>(&device, &config, consumer, active, draining, reset, err_fn)
        }
        SampleFormat::I16 => {
            build_typed::<i16>(&device, &config, consumer, active, draining, reset, err_fn)
        }
        SampleFormat::U16 => {
            build_typed::<u16>(&device, &config, consumer, active, draining, reset, err_fn)
        }
        other => Err(format!("unsupported sample format: {other:?}")),
    }
}

fn build_typed<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut consumer: HeapCons<f32>,
    active: Arc<AtomicBool>,
    draining: Arc<AtomicBool>,
    reset: Arc<AtomicBool>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                if reset.swap(false, Ordering::Relaxed) {
                    consumer.clear();
                }
                if !active.load(Ordering::Relaxed) {
                    for sample in data.iter_mut() {
                        *sample = T::from_sample(0.0f32);
                    }
                    return;
                }
                for frame in data.chunks_mut(CHANNELS as usize) {
                    let mut pair = [0.0f32; 2];
                    let count = consumer.pop_slice(&mut pair);
                    frame[0] = T::from_sample(pair[0]);
                    if frame.len() > 1 {
                        frame[1] = T::from_sample(if count > 1 { pair[1] } else { pair[0] });
                    }
                }
                if draining.load(Ordering::Relaxed) && consumer.is_empty() {
                    active.store(false, Ordering::Relaxed);
                    draining.store(false, Ordering::Relaxed);
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| e.to_string())
}

fn resolve_device(host: &cpal::Host, name: &str) -> Result<cpal::Device, AudioError> {
    let devices: Vec<cpal::Device> = host
        .output_devices()
        .map_err(|e| AudioError::Build(e.to_string()))?
        .collect();
    if devices.is_empty() {
        return Err(AudioError::NoDevices);
    }
    if let Some(device) = devices.iter().find(|d| {
        d.name()
            .map(|n| n.eq_ignore_ascii_case(name))
            .unwrap_or(false)
    }) {
        return Ok(device.clone());
    }
    let needle = name.to_lowercase();
    if let Some(device) = devices.iter().find(|d| {
        d.name()
            .map(|n| n.to_lowercase().contains(&needle))
            .unwrap_or(false)
    }) {
        return Ok(device.clone());
    }
    Err(AudioError::DeviceNotFound(name.to_string()))
}

fn pick_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, AudioError> {
    let ranges: Vec<_> = device
        .supported_output_configs()
        .map_err(|e| AudioError::Build(e.to_string()))?
        .collect();
    for format in [SampleFormat::F32, SampleFormat::I16, SampleFormat::U16] {
        if let Some(range) = ranges.iter().find(|range| {
            range.channels() == CHANNELS
                && range.sample_format() == format
                && range.min_sample_rate().0 <= OUTPUT_RATE
                && range.max_sample_rate().0 >= OUTPUT_RATE
        }) {
            return Ok(range.clone().with_sample_rate(SampleRate(OUTPUT_RATE)));
        }
    }
    let default = device
        .default_output_config()
        .map_err(|e| AudioError::Build(e.to_string()))?;
    if default.channels() == CHANNELS && default.sample_rate().0 == OUTPUT_RATE {
        return Ok(default);
    }
    Err(AudioError::UnsupportedConfig(format!(
        "{} (需要 48 kHz / 2ch)",
        device.name().unwrap_or_else(|_| "unknown".into())
    )))
}

/// Kept for the settings UI / diagnostics.
pub fn probe_device(name: &str) -> Result<String, String> {
    let host = cpal::default_host();
    let device = resolve_device(&host, name).map_err(|e| e.to_string())?;
    let config = pick_config(&device).map_err(|e| e.to_string())?;
    Ok(format!(
        "{} / {:.0} Hz / {} ch / {:?}",
        device.name().unwrap_or_default(),
        config.sample_rate().0,
        config.channels(),
        config.sample_format()
    ))
}
