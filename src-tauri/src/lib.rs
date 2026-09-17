use serde::{Deserialize, Serialize};
use tauri::Manager;
use std::{fs, path::PathBuf, sync::{Arc, Mutex}, thread, time::Duration};

#[derive(Clone, Serialize, Deserialize, Default)]
struct AppActivity { app: String, title: String, seconds: u64, activations: u64 }
#[derive(Clone, Serialize, Deserialize)]
struct TrackingState { enabled: bool, active_window: String, keyboard_actions: u64, mouse_actions: u64, app_activation_count: u64, focus_seconds: u64, hourly_focus: [u64; 24], apps: Vec<AppActivity> }
impl Default for TrackingState { fn default() -> Self { Self { enabled: true, active_window: String::new(), keyboard_actions: 0, mouse_actions: 0, app_activation_count: 0, focus_seconds: 0, hourly_focus: [0; 24], apps: Vec::new() } } }
type SharedState = Arc<Mutex<TrackingState>>;
type SharedPath = Arc<Mutex<PathBuf>>;

fn save(state: &TrackingState, path: &PathBuf) -> Result<(), String> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?; let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes).map_err(|e| e.to_string())?; fs::rename(temp, path).map_err(|e| e.to_string())
}
fn load(path: &PathBuf) -> TrackingState { fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default() }
fn path_value(path: &SharedPath) -> Result<PathBuf, String> { path.lock().map(|p| p.clone()).map_err(|_| "storage path unavailable".into()) }

#[tauri::command]
fn set_tracking(enabled: bool, state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<TrackingState, String> {
    let mut current = state.lock().map_err(|_| "tracking state unavailable")?; current.enabled = enabled; save(&current, &path_value(&path)?)?; Ok(current.clone())
}
#[tauri::command]
fn get_tracking_state(state: tauri::State<'_, SharedState>) -> Result<TrackingState, String> { state.lock().map(|s| s.clone()).map_err(|_| "tracking state unavailable".into()) }
#[tauri::command]
fn capture_snapshot(state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<TrackingState, String> {
    let mut current = state.lock().map_err(|_| "tracking state unavailable")?;
    #[cfg(windows)] if current.enabled { current.active_window = windows_active_window(); }
    save(&current, &path_value(&path)?)?; Ok(current.clone())
}
#[tauri::command]
fn clear_all_data(state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<(), String> {
    let mut current = state.lock().map_err(|_| "tracking state unavailable")?; let enabled = current.enabled; *current = TrackingState { enabled, ..Default::default() };
    let file = path_value(&path)?; if file.exists() { fs::remove_file(file).map_err(|e| e.to_string())?; } Ok(())
}

#[cfg(windows)]
fn windows_active_window() -> String {
    use windows::Win32::Foundation::HWND; use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
    let hwnd = unsafe { GetForegroundWindow() }; if hwnd == HWND::default() { return String::new(); }
    let mut buffer = [0u16; 512]; let len = unsafe { GetWindowTextW(hwnd, &mut buffer) }; String::from_utf16_lossy(&buffer[..len as usize])
}
#[cfg(windows)]
fn start_windows_tracker(state: SharedState, path: SharedPath) {
    use chrono::Timelike; use windows::Win32::Foundation::POINT; use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState; use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    thread::spawn(move || {
        let mut previous_window = String::new(); let mut previous_cursor = POINT { x: 0, y: 0 };
        loop {
            thread::sleep(Duration::from_secs(1)); let Ok(mut current) = state.lock() else { continue }; if !current.enabled { continue; }
            let active = windows_active_window();
            if active != previous_window && !active.is_empty() { current.app_activation_count += 1; previous_window = active.clone(); }
            current.active_window = active.clone(); current.focus_seconds += 1; current.hourly_focus[chrono::Local::now().hour() as usize] += 1;
            if let Some(item) = current.apps.iter_mut().find(|x| x.app == active) { item.seconds += 1; item.title = active.clone(); } else if !active.is_empty() { current.apps.push(AppActivity { app: active.clone(), title: active, seconds: 1, activations: 1 }); }
            let mut cursor = POINT { x: 0, y: 0 }; if unsafe { GetCursorPos(&mut cursor) }.is_ok() && (cursor.x != previous_cursor.x || cursor.y != previous_cursor.y) { current.mouse_actions += 1; previous_cursor = cursor; }
            for key in 0..=0xFE { if unsafe { GetAsyncKeyState(key) } < 0 { current.keyboard_actions += 1; break; } }
            let _ = path_value(&path).and_then(|p| save(&current, &p));
        }
    });
}
#[cfg(not(windows))] fn start_windows_tracker(_state: SharedState, _path: SharedPath) {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = Arc::new(Mutex::new(TrackingState::default())); let path = Arc::new(Mutex::new(PathBuf::new()));
    let state_for_setup = state.clone(); let path_for_setup = path.clone();
    tauri::Builder::default().setup(move |app| {
        let file = app.path().app_data_dir().map_err(|e| e.to_string())?.join("activity.json");
        if let Ok(mut s) = state_for_setup.lock() { *s = load(&file); } if let Ok(mut p) = path_for_setup.lock() { *p = file; }
        start_windows_tracker(state_for_setup.clone(), path_for_setup.clone()); Ok(())
    }).manage(state).manage(path).invoke_handler(tauri::generate_handler![set_tracking, get_tracking_state, capture_snapshot, clear_all_data]).run(tauri::generate_context!()).expect("error while running FlowLens");
}
