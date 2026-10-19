mod audio;
mod ble;
mod core;
mod keys;
mod recording;
mod state;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};

use state::{AppState, Settings, Snapshot};

#[tauri::command]
fn get_snapshot(app: AppHandle) -> Snapshot {
    app.state::<AppState>().snapshot()
}

#[tauri::command]
async fn start(app: AppHandle) -> Result<(), String> {
    state::start_engine(&app).await
}

#[tauri::command]
async fn stop(app: AppHandle) -> Result<(), String> {
    state::stop_engine(&app).await
}

#[tauri::command]
async fn update_settings(app: AppHandle, settings: Settings) -> Result<Snapshot, String> {
    let settings = settings.normalize();
    let was_running = {
        let state = app.state::<AppState>();
        state.engine.lock().is_some()
    };
    let previous = app.state::<AppState>().settings.lock().clone();
    state::save_settings(&app, &settings)?;
    {
        let state = app.state::<AppState>();
        *state.settings.lock() = settings.clone();
        let mut status = state.status.lock();
        status.audio_device = settings.audio_device.clone();
        let snapshot = status.clone();
        drop(status);
        let _ = app.emit("status", snapshot);
    }
    if let Err(err) = state::apply_key_mapping(&app) {
        app.state::<AppState>()
            .log("warn", format!("按键映射未应用: {err}"));
    }
    let _ = state::set_launch_at_login(&app, settings.launch_at_login).await;
    if was_running && previous.audio_device != settings.audio_device {
        state::stop_engine(&app).await?;
        state::start_engine(&app).await?;
    }
    Ok(app.state::<AppState>().snapshot())
}

#[tauri::command]
fn list_audio_devices() -> Vec<audio::AudioDeviceInfo> {
    audio::list_output_devices()
}

#[tauri::command]
fn refresh_devices(app: AppHandle) -> Snapshot {
    app.state::<AppState>().snapshot()
}

#[tauri::command]
fn apply_key_mapping(app: AppHandle) -> Result<(), String> {
    state::apply_key_mapping(&app)
}

#[tauri::command]
fn clear_key_mapping(app: AppHandle) {
    state::clear_key_mapping(&app)
}

#[tauri::command]
fn open_accessibility_settings() {
    keys::accessibility::open_settings();
}

#[tauri::command]
fn open_last_recording(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let state = app.state::<AppState>();
    let path = state
        .last_recording
        .lock()
        .clone()
        .ok_or_else(|| "还没有录音，先按一次语音键试试".to_string())?;
    app.opener()
        .open_path(path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_launch_at_login(app: AppHandle, enabled: bool) -> Result<Snapshot, String> {
    state::set_launch_at_login(&app, enabled).await?;
    {
        let state = app.state::<AppState>();
        state.settings.lock().launch_at_login = enabled;
        let settings = state.settings.lock().clone();
        state::save_settings(&app, &settings)?;
    }
    Ok(app.state::<AppState>().snapshot())
}

fn build_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "启动 / 停止语音桥接", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Xiaomi Remote", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle, &separator, &quit])?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => state::show_window(app),
            "toggle" => state::toggle_engine(app.clone()),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "xiaomi_remote_v2=info".into()),
        )
        .try_init();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            state::show_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let settings = state::load_settings(&handle);
            let key_mapper = keys::create();
            let state = AppState::new(settings.clone(), key_mapper);
            app.manage(state);

            if let Err(err) = state::apply_key_mapping(&handle) {
                handle
                    .state::<AppState>()
                    .log("warn", format!("按键映射未应用: {err}"));
            }
            build_tray(app)?;

            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            if settings.auto_start_engine {
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(err) = state::start_engine(&handle).await {
                        let state = handle.state::<AppState>();
                        state.log("error", err.clone());
                        let mut status = state.status.lock();
                        status.state = "error".into();
                        status.message = Some(err);
                        let snapshot = status.clone();
                        drop(status);
                        let _ = handle.emit("status", snapshot);
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            start,
            stop,
            update_settings,
            list_audio_devices,
            refresh_devices,
            apply_key_mapping,
            clear_key_mapping,
            open_accessibility_settings,
            open_last_recording,
            set_launch_at_login
        ])
        .build(tauri::generate_context!())
        .expect("error while building Xiaomi Remote");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            let handle = app_handle.clone();
            tauri::async_runtime::block_on(async move {
                state::shutdown(&handle).await;
            });
        }
    });
}
