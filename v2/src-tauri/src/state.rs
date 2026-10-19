use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, oneshot};
use tauri::async_runtime::JoinHandle;
use tracing::{error, info, warn};

use crate::audio::{self, AudioDeviceInfo, CpalSink};
use crate::ble::{self, EngineEvent};
use crate::core::atvv::SessionSnapshot;
use crate::core::AudioSink;
use crate::keys::{KeyMapSettings, KeyMapper};
use crate::recording::RecordingSink;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub audio_device: String,
    pub key_map: KeyMapSettings,
    pub auto_start_engine: bool,
    pub launch_at_login: bool,
    pub verbose_logs: bool,
    pub record_last_session: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            audio_device: audio::default_device_hint().to_string(),
            key_map: KeyMapSettings::default(),
            auto_start_engine: true,
            launch_at_login: false,
            verbose_logs: false,
            record_last_session: true,
        }
    }
}

impl Settings {
    pub fn normalize(mut self) -> Self {
        if self.audio_device.trim().is_empty() {
            self.audio_device = audio::default_device_hint().to_string();
        }
        self
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: &'static str,
    pub message: String,
}

#[derive(Default)]
pub struct LogBuffer {
    entries: Mutex<VecDeque<LogEntry>>,
}

impl LogBuffer {
    pub fn push(&self, level: &'static str, message: impl Into<String>) {
        let mut entries = self.entries.lock();
        if entries.len() >= 500 {
            entries.pop_front();
        }
        entries.push_back(LogEntry {
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            level,
            message: message.into(),
        });
    }

    pub fn snapshot(&self) -> Vec<LogEntry> {
        self.entries.lock().iter().cloned().collect()
    }
}

pub fn idle_session() -> SessionSnapshot {
    SessionSnapshot {
        state: "idle",
        mic_open: false,
        audio_frames: 0,
        pcm_samples: 0,
        gate_gain: 1.0,
        caps: None,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineStatus {
    pub running: bool,
    pub state: String,
    pub message: Option<String>,
    pub device: Option<String>,
    pub session: SessionSnapshot,
    pub audio_device: String,
    pub key_backend: String,
    pub key_supported: bool,
}

impl EngineStatus {
    pub fn idle(settings: &Settings, key: &dyn KeyMapper) -> Self {
        Self {
            running: false,
            state: "idle".into(),
            message: None,
            device: None,
            session: idle_session(),
            audio_device: settings.audio_device.clone(),
            key_backend: key.backend().to_string(),
            key_supported: key.supported(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub settings: Settings,
    pub status: EngineStatus,
    pub devices: Vec<AudioDeviceInfo>,
    pub logs: Vec<LogEntry>,
    pub default_device: String,
    pub platform: String,
    pub last_recording: Option<String>,
    pub accessibility_trusted: bool,
}

pub struct EngineHandle {
    stop: Option<oneshot::Sender<()>>,
    tasks: Vec<JoinHandle<()>>,
}

impl EngineHandle {
    pub async fn shutdown(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        for task in self.tasks.drain(..) {
            let _ = tokio::time::timeout(Duration::from_secs(5), task).await;
        }
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub status: Mutex<EngineStatus>,
    pub engine: Mutex<Option<EngineHandle>>,
    pub key_mapper: Box<dyn KeyMapper>,
    pub logs: Arc<LogBuffer>,
    pub last_recording: Arc<Mutex<Option<PathBuf>>>,
}

impl AppState {
    pub fn new(settings: Settings, key_mapper: Box<dyn KeyMapper>) -> Self {
        let status = EngineStatus::idle(&settings, key_mapper.as_ref());
        Self {
            settings: Mutex::new(settings),
            status: Mutex::new(status),
            engine: Mutex::new(None),
            key_mapper,
            logs: Arc::new(LogBuffer::default()),
            last_recording: Arc::new(Mutex::new(None)),
        }
    }

    pub fn log(&self, level: &'static str, message: impl Into<String>) {
        let message = message.into();
        match level {
            "error" => error!("{message}"),
            "warn" => warn!("{message}"),
            _ => info!("{message}"),
        }
        self.logs.push(level, message);
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            settings: self.settings.lock().clone(),
            status: self.status.lock().clone(),
            devices: audio::list_output_devices(),
            logs: self.logs.snapshot(),
            default_device: audio::default_device_hint().to_string(),
            platform: std::env::consts::OS.to_string(),
            last_recording: self
                .last_recording
                .lock()
                .as_ref()
                .map(|path| path.display().to_string()),
            accessibility_trusted: crate::keys::accessibility::is_trusted(),
        }
    }
}

pub fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("config dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create config dir: {e}"))?;
    Ok(dir.join("settings.json"))
}

pub fn load_settings(app: &AppHandle) -> Settings {
    let Ok(path) = settings_path(app) else {
        return Settings::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<Settings>(&text)
            .unwrap_or_default()
            .normalize(),
        Err(_) => Settings::default(),
    }
}

pub fn save_settings(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("write settings: {e}"))
}

pub fn apply_key_mapping(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().clone();
    match state.key_mapper.apply(&settings.key_map) {
        Ok(()) => {
            let backend = state.key_mapper.backend().to_string();
            let supported = state.key_mapper.supported();
            {
                let mut status = state.status.lock();
                status.key_backend = backend.clone();
                status.key_supported = supported;
                let snapshot = status.clone();
                drop(status);
                let _ = app.emit("status", snapshot);
            }
            state.log("info", format!("按键映射已应用（{backend}）"));
            Ok(())
        }
        Err(err) => {
            state.log("warn", format!("按键映射未应用: {err}"));
            Err(err)
        }
    }
}

pub fn clear_key_mapping(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Err(err) = state.key_mapper.clear() {
        state.log("warn", format!("清除按键映射失败: {err}"));
    }
}

pub async fn start_engine(app: &AppHandle) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        if state.engine.lock().is_some() {
            return Ok(());
        }
    }

    let settings = app.state::<AppState>().settings.lock().clone();
    let state = app.state::<AppState>();

    state.log("info", "正在启动语音桥接…");
    let sink = Arc::new(
        CpalSink::new(&settings.audio_device).map_err(|e| format!("音频设备不可用: {e}"))?,
    );
    state.log(
        "info",
        format!("音频输出: {} ({} Hz)", sink.device_name(), 48_000),
    );

    if let Err(err) = state.key_mapper.apply(&settings.key_map) {
        state.log("warn", format!("按键映射未应用: {err}"));
    }

    let recording_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());
    let recording = Arc::new(RecordingSink::new(
        sink,
        recording_dir,
        settings.record_last_session,
        state.last_recording.clone(),
    ));
    let output: Arc<dyn AudioSink> = recording;
    let (event_tx, mut event_rx) = mpsc::channel::<EngineEvent>(128);
    let (stop_tx, stop_rx) = oneshot::channel::<()>();

    let ble_task = tauri::async_runtime::spawn(ble::run(output, event_tx, stop_rx));
    let app_for_events = app.clone();
    let event_task = tauri::async_runtime::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            handle_engine_event(&app_for_events, event);
        }
    });

    {
        let state = app.state::<AppState>();
        let mut status = state.status.lock();
        status.running = true;
        status.state = "starting".into();
        status.message = None;
        status.audio_device = settings.audio_device.clone();
        let snapshot = status.clone();
        drop(status);
        let _ = app.emit("status", snapshot);
        let mut engine = state.engine.lock();
        *engine = Some(EngineHandle {
            stop: Some(stop_tx),
            tasks: vec![ble_task, event_task],
        });
    }
    Ok(())
}

pub async fn stop_engine(app: &AppHandle) -> Result<(), String> {
    let handle = {
        let state = app.state::<AppState>();
        state.engine.lock().take()
    };
    if let Some(handle) = handle {
        handle.shutdown().await;
    }
    let state = app.state::<AppState>();
    {
        let mut status = state.status.lock();
        status.running = false;
        status.state = "idle".into();
        status.message = None;
        status.device = None;
        status.session = idle_session();
        let snapshot = status.clone();
        drop(status);
        let _ = app.emit("status", snapshot);
    }
    state.log("info", "语音桥接已停止");
    Ok(())
}

pub async fn shutdown(app: &AppHandle) {
    let _ = stop_engine(app);
    clear_key_mapping(app);
    let state = app.state::<AppState>();
    let settings = state.settings.lock().clone();
    let _ = save_settings(app, &settings);
}

fn handle_engine_event(app: &AppHandle, event: EngineEvent) {
    let state = app.state::<AppState>();
    match &event {
        EngineEvent::Status {
            state: new_state,
            message,
            device,
        } => {
            let mut status = state.status.lock();
            status.state = new_state.clone();
            status.message = message.clone();
            status.device = device.clone();
            status.running = true;
            let snapshot = status.clone();
            drop(status);
            if let Some(message) = message {
                state.log("info", message.clone());
            }
            state.log("info", format!("状态: {new_state}"));
            let _ = app.emit("status", snapshot);
            if new_state == "connected" {
                // hidutil mappings are per-device and can be lost when the
                // remote reconnects; re-apply after every successful connect.
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = apply_key_mapping(&app);
                });
            }
        }
        EngineEvent::Session(session) => {
            let mut status = state.status.lock();
            status.session = session.clone();
            let snapshot = status.clone();
            drop(status);
            let _ = app.emit("status", snapshot);
        }
        EngineEvent::Error { message } => {
            state.log("error", message.clone());
            let mut status = state.status.lock();
            status.state = "error".into();
            status.message = Some(message.clone());
            let snapshot = status.clone();
            drop(status);
            let _ = app.emit("status", snapshot);
        }
    }
    let _ = app.emit("engine-event", event);
}

pub async fn set_launch_at_login(app: &AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let autolaunch = app.autolaunch();
    if enabled {
        autolaunch.enable().map_err(|e| e.to_string())
    } else {
        autolaunch.disable().map_err(|e| e.to_string())
    }
}

pub fn toggle_engine(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let running = {
            let state = app.state::<AppState>();
            state.engine.lock().is_some()
        };
        let result = if running {
            stop_engine(&app).await
        } else {
            start_engine(&app).await
        };
        if let Err(err) = result {
            let state = app.state::<AppState>();
            state.log("error", err.clone());
            let mut status = state.status.lock();
            status.state = "error".into();
            status.message = Some(err);
            let snapshot = status.clone();
            drop(status);
            let _ = app.emit("status", snapshot);
        }
    });
}

pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
