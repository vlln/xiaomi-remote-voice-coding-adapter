//! ATVV protocol and voice session state machine.
//!
//! Ported from V1 `VoiceCaptureDelegate`, with the BLE transport removed so the
//! state machine can be unit-tested with a mock `AudioSink`.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tracing::{debug, info, warn};

use super::adpcm::AdpcmDecoder;
use super::dsp::{apply_gain, NoiseGate, NoiseReducer};
use super::AudioSink;

pub const VOICE_SERVICE: &str = "ab5e0001-5a21-4f05-bc7d-af01f617b664";
pub const VOICE_TX: &str = "ab5e0002-5a21-4f05-bc7d-af01f617b664";
pub const VOICE_AUDIO: &str = "ab5e0003-5a21-4f05-bc7d-af01f617b664";
pub const VOICE_CONTROL: &str = "ab5e0004-5a21-4f05-bc7d-af01f617b664";

pub const GET_CAPS: [u8; 6] = [0x0A, 0x01, 0x00, 0x00, 0x03, 0x03];

pub const CONTROL_QUIET: Duration = Duration::from_millis(300);
pub const START_CONFIRM: Duration = Duration::from_millis(100);
pub const AUDIO_SILENCE_TIMEOUT: Duration = Duration::from_secs(2);
pub const TAIL_GUARD_FRAMES: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Idle,
    Armed,
    Streaming,
    Closing,
}

impl SessionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Armed => "armed",
            Self::Streaming => "streaming",
            Self::Closing => "closing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Caps {
    pub version: u16,
    pub codecs: u8,
    pub interaction: u8,
    pub frame_size: usize,
    pub selected_codec: u8,
    pub sample_rate: u32,
}

impl Caps {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 7 || data[0] != 0x0B {
            return None;
        }
        let version = u16::from_be_bytes([data[1], data[2]]);
        let (codecs, interaction, frame_size) = if version >= 0x0100 {
            let mut codecs = data[3];
            let mut interaction = data[4];
            let frame_size = u16::from_be_bytes([data[5], data[6]]) as usize;
            if codecs == 0 && data.len() >= 9 && data[4] & 0x03 != 0 {
                codecs = data[4];
                interaction = 0x03;
            }
            (codecs, interaction, frame_size)
        } else {
            if data.len() < 9 {
                return None;
            }
            let codecs = data[4];
            let frame_size = u16::from_be_bytes([data[5], data[6]]) as usize;
            (codecs, 0x00, frame_size)
        };
        let selected_codec = if codecs & 0x02 != 0 { 0x02 } else { 0x01 };
        Some(Self {
            version,
            codecs,
            interaction,
            frame_size: frame_size.max(1),
            selected_codec,
            sample_rate: if selected_codec == 0x02 { 16_000 } else { 8_000 },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCommand {
    MicOpen(Vec<u8>),
    MicClose(Vec<u8>),
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSnapshot {
    pub state: &'static str,
    pub mic_open: bool,
    pub audio_frames: u64,
    pub pcm_samples: u64,
    pub gate_gain: f64,
    pub caps: Option<Caps>,
}

pub struct Session {
    state: SessionState,
    caps: Option<Caps>,
    decoder: AdpcmDecoder,
    noise_reducer: NoiseReducer,
    noise_gate: NoiseGate,
    pending: Vec<u8>,
    tail_guard: VecDeque<(Vec<i16>, Vec<i16>)>,
    skip_frames: u32,
    mic_open: bool,
    connected: bool,
    audio_frames: u64,
    pcm_samples: u64,
    session_start: Instant,
    last_audio: Instant,
    closing_activity: Instant,
    output: Arc<dyn AudioSink>,
}

impl Session {
    pub fn new(output: Arc<dyn AudioSink>) -> Self {
        let now = Instant::now();
        Self {
            state: SessionState::Idle,
            caps: None,
            decoder: AdpcmDecoder::new(),
            noise_reducer: NoiseReducer::new(),
            noise_gate: NoiseGate::new(),
            pending: Vec::with_capacity(256),
            tail_guard: VecDeque::with_capacity(TAIL_GUARD_FRAMES + 2),
            skip_frames: 0,
            mic_open: false,
            connected: true,
            audio_frames: 0,
            pcm_samples: 0,
            session_start: now,
            last_audio: now,
            closing_activity: now,
            output,
        }
    }

    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            state: self.state.as_str(),
            mic_open: self.mic_open,
            audio_frames: self.audio_frames,
            pcm_samples: self.pcm_samples,
            gate_gain: self.noise_gate.gain(),
            caps: self.caps,
        }
    }

    /// Feed a control characteristic notification.
    pub fn on_control(&mut self, data: &[u8]) -> Vec<SessionCommand> {
        if data.is_empty() {
            return Vec::new();
        }
        let opcode = data[0];
        debug!(
            opcode = format!("0x{opcode:02X}"),
            len = data.len(),
            state = self.state.as_str(),
            mic_open = self.mic_open,
            "ATVV control"
        );

        match opcode {
            0x0B => {
                if let Some(caps) = Caps::parse(data) {
                    info!(?caps, "ATVV capabilities");
                    self.caps = Some(caps);
                }
                Vec::new()
            }
            0x08 => {
                let now = Instant::now();
                // A new voice-key press is a real user action. Residual
                // AUDIO_START/AUDIO_STOP packets from the previous session must
                // not keep us in CLOSING and swallow the press.
                if self.state == SessionState::Closing {
                    info!("voice key pressed while closing; re-arming");
                    self.state = SessionState::Idle;
                    self.closing_activity = now;
                }
                info!("voice key pressed");
                if self.caps.is_some() && self.state == SessionState::Idle {
                    let cmd = self.begin_mic_open();
                    self.state = SessionState::Armed;
                    cmd.into_iter().collect()
                } else {
                    Vec::new()
                }
            }
            0x04 => {
                let now = Instant::now();
                if self.state == SessionState::Closing {
                    let quiet_for = now.saturating_duration_since(self.closing_activity);
                    if quiet_for < CONTROL_QUIET {
                        debug!(
                            quiet_ms = quiet_for.as_millis() as u64,
                            "AUDIO_START ignored while closing"
                        );
                        return Vec::new();
                    }
                    info!("AUDIO_START after closing; re-arming");
                    self.state = SessionState::Idle;
                }
                if self.state == SessionState::Streaming {
                    let since_open = now.saturating_duration_since(self.session_start);
                    if since_open <= START_CONFIRM {
                        debug!("AUDIO_START confirmation");
                    } else {
                        warn!(
                            audio_frames = self.audio_frames,
                            pending = self.pending.len(),
                            "duplicate AUDIO_START ignored"
                        );
                    }
                    return Vec::new();
                }

                let mut commands = Vec::new();
                if self.state == SessionState::Idle {
                    if let Some(cmd) = self.begin_mic_open() {
                        commands.push(cmd);
                    } else {
                        return commands;
                    }
                }
                self.state = SessionState::Streaming;
                self.decoder.reset(0, 0);
                self.pending.clear();
                self.tail_guard.clear();
                self.skip_frames = 8;
                info!("audio stream started");
                commands
            }
            0x0A if data.len() >= 7 => {
                let predictor = i16::from_be_bytes([data[4], data[5]]) as i32;
                let step_index = data[6];
                self.decoder.reset(predictor, step_index);
                debug!(predictor, step_index, "audio sync");
                Vec::new()
            }
            0x00 => {
                if self.state == SessionState::Closing {
                    debug!("AUDIO_STOP ignored while closing");
                    return Vec::new();
                }
                if !self.mic_open {
                    debug!("duplicate AUDIO_STOP ignored");
                    return Vec::new();
                }
                info!("voice session ended by remote");
                self.end_speech()
            }
            _ => Vec::new(),
        }
    }

    /// Feed an audio characteristic notification.
    pub fn on_audio(&mut self, data: &[u8]) {
        if self.state != SessionState::Streaming
            || !self.mic_open
            || self.caps.is_none()
            || !self.connected
        {
            return;
        }
        let Some(caps) = self.caps else { return };
        self.last_audio = Instant::now();
        self.pending.extend_from_slice(data);
        let frame_size = caps.frame_size;

        while self.pending.len() >= frame_size {
            if !self.mic_open {
                break;
            }
            let frame: Vec<u8> = self.pending.drain(..frame_size).collect();
            self.audio_frames += 1;

            if self.skip_frames > 0 {
                self.skip_frames -= 1;
                continue;
            }

            let raw = self.decoder.decode_bytes(&frame);
            let noise_reduced = self.noise_reducer.process(&raw);
            let gated = self.noise_gate.process(&noise_reduced);
            let gained = apply_gain(&gated, 15.0);
            self.queue_processed_frame(raw, gained);
        }
    }

    /// Called by the transport when writing MIC_OPEN failed.
    pub fn mic_open_failed(&mut self) {
        self.mic_open = false;
        self.state = SessionState::Idle;
        self.output.stop(false);
    }

    /// Periodic watchdog tick. `now` is injectable for tests.
    pub fn tick(&mut self, now: Instant) -> Vec<SessionCommand> {
        if !self.mic_open {
            return Vec::new();
        }
        if !self.connected {
            warn!("watchdog: BLE disconnected, ending session");
            return self.end_speech();
        }
        if now.saturating_duration_since(self.last_audio) > AUDIO_SILENCE_TIMEOUT {
            warn!(
                silence_ms = now.saturating_duration_since(self.last_audio).as_millis() as u64,
                "watchdog: audio silence timeout"
            );
            return self.end_speech();
        }
        Vec::new()
    }

    pub fn on_disconnect(&mut self) {
        self.connected = false;
        self.mic_open = false;
        self.state = SessionState::Idle;
        self.closing_activity = Instant::now();
        self.tail_guard.clear();
        self.output.stop(false);
    }

    fn begin_mic_open(&mut self) -> Option<SessionCommand> {
        if !self.connected {
            warn!("MIC_OPEN skipped: not connected");
            return None;
        }
        self.output.start();
        let version = self.caps.map_or(0x0100, |c| c.version);
        let payload = if version >= 0x0100 {
            vec![0x0C, 0x00]
        } else {
            vec![0x0C, 0x00, 0x02]
        };
        self.mic_open = true;
        self.session_start = Instant::now();
        self.last_audio = self.session_start;
        info!(payload = hex(&payload), "MIC_OPEN");
        Some(SessionCommand::MicOpen(payload))
    }

    fn mic_close_command(&self) -> SessionCommand {
        let version = self.caps.map_or(0x0100, |c| c.version);
        let payload = if version >= 0x0100 {
            vec![0x0D, 0x00]
        } else {
            vec![0x0D]
        };
        SessionCommand::MicClose(payload)
    }

    pub fn end_speech(&mut self) -> Vec<SessionCommand> {
        if !self.mic_open {
            return Vec::new();
        }
        self.mic_open = false;
        self.state = SessionState::Closing;
        self.closing_activity = Instant::now();

        // Flush a partial frame, matching V1's zero-padding behaviour.
        if let (Some(caps), false) = (self.caps, self.pending.is_empty()) {
            let frame_size = caps.frame_size;
            let pad = (frame_size - self.pending.len() % frame_size) % frame_size;
            if pad > 0 {
                self.pending.extend(std::iter::repeat_n(0, pad));
            }
            let mut tail_frames = 0usize;
            while self.pending.len() >= frame_size {
                let frame: Vec<u8> = self.pending.drain(..frame_size).collect();
                let raw = self.decoder.decode_bytes(&frame);
                let noise_reduced = self.noise_reducer.process(&raw);
                let gated = self.noise_gate.process(&noise_reduced);
                let gained = apply_gain(&gated, 15.0);
                self.queue_processed_frame(raw, gained);
                tail_frames += 1;
            }
            debug!(tail_frames, "flushed tail frames");
        }

        let finished_frames = self.audio_frames;
        let finished_samples = self.pcm_samples;
        info!(frames = finished_frames, samples = finished_samples, "voice session finished");
        self.pending.clear();
        self.skip_frames = 0;
        self.tail_guard.clear();
        self.audio_frames = 0;
        self.pcm_samples = 0;
        self.noise_reducer = NoiseReducer::new();
        self.noise_gate = NoiseGate::new();
        self.output.stop(true);
        let close = self.mic_close_command();
        info!(payload = hex(command_payload(&close)), "MIC_CLOSE");
        vec![close]
    }

    fn queue_processed_frame(&mut self, raw: Vec<i16>, processed: Vec<i16>) {
        self.tail_guard.push_back((raw, processed));
        if self.tail_guard.len() <= TAIL_GUARD_FRAMES {
            return;
        }
        if let Some((_raw, samples)) = self.tail_guard.pop_front() {
            self.pcm_samples += samples.len() as u64;
            self.output.write(&samples);
        }
    }
}

fn command_payload(command: &SessionCommand) -> &[u8] {
    match command {
        SessionCommand::MicOpen(payload) | SessionCommand::MicClose(payload) => payload,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    #[derive(Default)]
    struct MockSink {
        started: Mutex<u32>,
        stopped: Mutex<Vec<bool>>,
        samples: Mutex<Vec<i16>>,
    }

    impl AudioSink for MockSink {
        fn start(&self) {
            *self.started.lock() += 1;
        }
        fn write(&self, samples: &[i16]) {
            self.samples.lock().extend_from_slice(samples);
        }
        fn stop(&self, drain: bool) {
            self.stopped.lock().push(drain);
        }
    }

    fn caps_payload() -> Vec<u8> {
        vec![0x0B, 0x01, 0x00, 0x03, 0x03, 0x00, 0x78]
    }

    #[test]
    fn parses_caps() {
        let caps = Caps::parse(&caps_payload()).unwrap();
        assert_eq!(caps.version, 0x0100);
        assert_eq!(caps.codecs, 3);
        assert_eq!(caps.interaction, 3);
        assert_eq!(caps.frame_size, 120);
        assert_eq!(caps.selected_codec, 2);
        assert_eq!(caps.sample_rate, 16_000);
    }

    #[test]
    fn parses_legacy_caps() {
        let caps = Caps::parse(&[0x0B, 0x00, 0x01, 0x00, 0x03, 0x00, 0x78, 0x00, 0x00]).unwrap();
        assert_eq!(caps.version, 0x0001);
        assert_eq!(caps.codecs, 3);
        assert_eq!(caps.frame_size, 120);
        assert_eq!(caps.selected_codec, 2);
    }

    #[test]
    fn session_flow_sends_mic_commands_and_audio() {
        let sink = Arc::new(MockSink::default());
        let mut session = Session::new(sink.clone());
        assert!(session.on_control(&caps_payload()).is_empty());
        assert_eq!(session.state(), SessionState::Idle);

        let commands = session.on_control(&[0x08]);
        assert_eq!(commands.len(), 1);
        assert!(matches!(&commands[0], SessionCommand::MicOpen(p) if p == &[0x0C, 0x00]));
        assert_eq!(session.state(), SessionState::Armed);
        assert_eq!(*sink.started.lock(), 1);

        let commands = session.on_control(&[0x04]);
        assert_eq!(commands.len(), 0); // already open, only state changes
        assert_eq!(session.state(), SessionState::Streaming);

        // 8 skipped frames, then 100 audio frames; tail guard keeps 2.
        for _ in 0..108 {
            session.on_audio(&[0x00; 120]);
        }
        assert_eq!(session.snapshot().audio_frames, 108);
        assert!(session.snapshot().pcm_samples > 0);
        assert!(!sink.samples.lock().is_empty());

        let commands = session.on_control(&[0x00]);
        assert_eq!(commands.len(), 1);
        assert!(matches!(&commands[0], SessionCommand::MicClose(p) if p == &[0x0D, 0x00]));
        assert_eq!(session.state(), SessionState::Closing);
        assert_eq!(*sink.stopped.lock(), vec![true]);
    }

    #[test]
    fn audio_before_streaming_is_ignored() {
        let sink = Arc::new(MockSink::default());
        let mut session = Session::new(sink.clone());
        session.on_audio(&[0x00; 120]);
        assert!(sink.samples.lock().is_empty());
        assert_eq!(session.snapshot().audio_frames, 0);
    }
}
