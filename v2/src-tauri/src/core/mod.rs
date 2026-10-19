pub mod adpcm;
pub mod atvv;
pub mod dsp;

/// Destination for processed 16 kHz mono PCM.
///
/// The audio backend owns resampling/format conversion and must be cheap to call
/// from the BLE notification task.
pub trait AudioSink: Send + Sync {
    fn start(&self);
    fn write(&self, samples: &[i16]);
    fn stop(&self, drain: bool);
}
