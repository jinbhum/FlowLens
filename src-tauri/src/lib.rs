use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::{thread, time::Duration};

#[derive(Clone, Serialize)]
struct TrackingState {
    enabled: bool,
    active_window: String,
    keyboard_actions: u64,
    mouse_actions: u64,
    app_activation_count: u64,
    focus_seconds: u64,
}

#[tauri::command]
fn set_tracking(enabled: bool, state: tauri::State<'_, Arc<Mutex<TrackingState>>>) -> Result<TrackingState, String> {
    let mut current = state.lock().map_err(|_| "tracking state unavailable")?;
    current.enabled = enabled;
    Ok(current.clone())
}

#[tauri::command]
fn get_tracking_state(state: tauri::State<'_, Arc<Mutex<TrackingState>>>) -> Result<TrackingState, String> {
    let current = state.lock().map_err(|_| "tracking state unavailable")?;
    Ok(current.clone())
}

#[tauri::command]
fn capture_snapshot(state: tauri::State<'_, Arc<Mutex<TrackingState>>>) -> Result<TrackingState, String> {
    let mut current = state.lock().map_err(|_| "tracking state unavailable")?;
    if current.enabled {
        #[cfg(windows)]
        { current.active_window = windows_active_window(); }
    }
    Ok(current.clone())
}

#[cfg(windows)]
fn windows_active_window() -> String {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd == HWND::default() { return String::new(); }
    let mut buffer = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..len as usize])
}

#[cfg(windows)]
fn start_windows_tracker(state: Arc<Mutex<TrackingState>>) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    thread::spawn(move || {
        let mut previous_window = String::new();
        let mut previous_cursor = POINT { x: 0, y: 0 };
        loop {
            thread::sleep(Duration::from_secs(1));
            let Ok(mut current) = state.lock() else { continue };
            if !current.enabled { continue; }
            let active = windows_active_window();
            if active != previous_window && !active.is_empty() {
                current.app_activation_count += 1;
                previous_window = active.clone();
            }
            current.active_window = active;
            current.focus_seconds += 1;
            let mut cursor = POINT { x: 0, y: 0 };
            if unsafe { GetCursorPos(&mut cursor) }.is_ok() && (cursor.x != previous_cursor.x || cursor.y != previous_cursor.y) {
                current.mouse_actions += 1;
                previous_cursor = cursor;
            }
            // Record only whether keyboard input occurred; never record key codes or text.
            for key in 0..=0xFE {
                if unsafe { GetAsyncKeyState(key) } < 0 { current.keyboard_actions += 1; break; }
            }
        }
    });
}

#[cfg(not(windows))]
fn start_windows_tracker(_state: Arc<Mutex<TrackingState>>) {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = Arc::new(Mutex::new(TrackingState {
        enabled: true,
        active_window: String::new(),
        keyboard_actions: 0,
        mouse_actions: 0,
        app_activation_count: 0,
        focus_seconds: 0,
    }));
    start_windows_tracker(state.clone());
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![set_tracking, get_tracking_state, capture_snapshot])
        .run(tauri::generate_context!())
        .expect("error while running FlowLens");
}
