//! Optional per-session WAV recording.
//!
//! The recording is a diagnostic aid: it writes the processed 16 kHz mono PCM
//! (the exact samples sent to the virtual microphone) to a fixed `last_session`
//! file so the UI can replay it without routing through an audio app.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::Arc;

use hound::{SampleFormat, WavSpec, WavWriter};
use parking_lot::Mutex;
use tracing::{info, warn};

use crate::core::AudioSink;

const SAMPLE_RATE: u32 = 16_000;

pub struct RecordingSink {
    inner: Arc<dyn AudioSink>,
    dir: PathBuf,
    enabled: bool,
    writer: Mutex<Option<WavWriter<BufWriter<File>>>>,
    last_path: Arc<Mutex<Option<PathBuf>>>,
}

impl RecordingSink {
    pub fn new(
        inner: Arc<dyn AudioSink>,
        dir: PathBuf,
        enabled: bool,
        last_path: Arc<Mutex<Option<PathBuf>>>,
    ) -> Self {
        Self {
            inner,
            dir,
            enabled,
            writer: Mutex::new(None),
            last_path,
        }
    }

    fn begin(&self) {
        if !self.enabled {
            return;
        }
        let mut writer = self.writer.lock();
        if writer.is_some() {
            return;
        }
        if let Err(err) = fs::create_dir_all(&self.dir) {
            warn!(%err, "could not create recording directory");
            return;
        }
        let path = self.dir.join("last_session.wav");
        let spec = WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        match WavWriter::create(&path, spec) {
            Ok(wav) => {
                info!(path = %path.display(), "recording voice session");
                *self.last_path.lock() = Some(path);
                *writer = Some(wav);
            }
            Err(err) => warn!(%err, "could not create recording"),
        }
    }

    fn finish(&self) {
        if let Some(wav) = self.writer.lock().take() {
            if let Err(err) = wav.finalize() {
                warn!(%err, "could not finalize recording");
            }
        }
    }
}

impl AudioSink for RecordingSink {
    fn start(&self) {
        self.begin();
        self.inner.start();
    }

    fn write(&self, samples: &[i16]) {
        if self.enabled {
            if let Some(wav) = self.writer.lock().as_mut() {
                for &sample in samples {
                    let _ = wav.write_sample(sample);
                }
            }
        }
        self.inner.write(samples);
    }

    fn stop(&self, drain: bool) {
        self.finish();
        self.inner.stop(drain);
    }
}

impl Drop for RecordingSink {
    fn drop(&mut self) {
        self.finish();
    }
}
