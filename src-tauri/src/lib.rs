use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use tauri::Manager;

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct AppActivity {
    app: String,
    title: String,
    seconds: u64,
    activations: u64,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct TimelineEvent {
    at: String,
    app: String,
    title: String,
    kind: String,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct MinuteSnapshot {
    at: String,
    active_app_count: u64,
    focus_seconds: u64,
    keyboard_actions: u64,
    mouse_actions: u64,
    mouse_distance_px: f64,
    network_rx_bytes: u64,
    network_tx_bytes: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct TrackingState {
    enabled: bool,
    active_window: String,
    keyboard_actions: u64,
    mouse_actions: u64,
    app_activation_count: u64,
    focus_seconds: u64,
    hourly_focus: [u64; 24],
    apps: Vec<AppActivity>,
    timeline: Vec<TimelineEvent>,
    minute_snapshots: Vec<MinuteSnapshot>,
    active_app_count: u64,
    mouse_distance_px: f64,
    mouse_clicks: u64,
    avg_click_interval_ms: u64,
    network_rx_bytes: u64,
    network_tx_bytes: u64,
    notification_count: u64,
}

impl Default for TrackingState {
    fn default() -> Self {
        Self {
            enabled: true,
            active_window: String::new(),
            keyboard_actions: 0,
            mouse_actions: 0,
            app_activation_count: 0,
            focus_seconds: 0,
            hourly_focus: [0; 24],
            apps: Vec::new(),
            timeline: Vec::new(),
            minute_snapshots: Vec::new(),
            active_app_count: 0,
            mouse_distance_px: 0.0,
            mouse_clicks: 0,
            avg_click_interval_ms: 0,
            network_rx_bytes: 0,
            network_tx_bytes: 0,
            notification_count: 0,
        }
    }
}

type SharedState = Arc<Mutex<TrackingState>>;
type SharedPath = Arc<Mutex<PathBuf>>;

fn save(state: &TrackingState, path: &PathBuf) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    fs::rename(temp, path).map_err(|e| e.to_string())
}

fn load(path: &PathBuf) -> TrackingState {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn storage_path(path: &SharedPath) -> Result<PathBuf, String> {
    path.lock()
        .map(|value| value.clone())
        .map_err(|_| "storage path unavailable".into())
}

#[tauri::command]
fn set_tracking(
    enabled: bool,
    state: tauri::State<'_, SharedState>,
    path: tauri::State<'_, SharedPath>,
) -> Result<TrackingState, String> {
    let mut current = state.lock().map_err(|_| "state unavailable")?;
    current.enabled = enabled;
    save(&current, &storage_path(&path)?)?;
    Ok(current.clone())
}

#[tauri::command]
fn get_tracking_state(
    state: tauri::State<'_, SharedState>,
) -> Result<TrackingState, String> {
    state
        .lock()
        .map(|value| value.clone())
        .map_err(|_| "state unavailable".into())
}

#[tauri::command]
fn clear_all_data(
    state: tauri::State<'_, SharedState>,
    path: tauri::State<'_, SharedPath>,
) -> Result<(), String> {
    let mut current = state.lock().map_err(|_| "state unavailable")?;
    let enabled = current.enabled;
    *current = TrackingState {
        enabled,
        ..Default::default()
    };
    let file = storage_path(&path)?;
    if file.exists() {
        fs::remove_file(file).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(windows)]
fn active_window() -> String {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd == HWND::default() {
        return String::new();
    }
    let mut buffer = [0u16; 512];
    let length = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..length as usize])
}

#[cfg(windows)]
unsafe extern "system" fn count_visible_window(
    hwnd: windows::Win32::Foundation::HWND,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::BOOL {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, IsWindowVisible};

    if IsWindowVisible(hwnd).as_bool() && GetWindowTextLengthW(hwnd) > 0 {
        let count = &mut *(lparam.0 as *mut u64);
        *count += 1;
    }
    BOOL(1)
}

#[cfg(windows)]
fn visible_app_count() -> u64 {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::EnumWindows;

    let mut count = 0u64;
    let _ = unsafe {
        EnumWindows(
            Some(count_visible_window),
            LPARAM((&mut count as *mut u64) as isize),
        )
    };
    count
}

#[cfg(windows)]
fn network_totals() -> (u64, u64) {
    use std::ffi::c_void;
    use windows::Win32::NetworkManagement::IpHelper::{
        FreeMibTable, GetIfTable2, MIB_IF_TABLE2,
    };

    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    let result = unsafe { GetIfTable2(&mut table) };
    if result.0 != 0 || table.is_null() {
        return (0, 0);
    }

    let table_ref = unsafe { &*table };
    let first_row = table_ref.Table.as_ptr();
    let mut received = 0u64;
    let mut sent = 0u64;
    for index in 0..table_ref.NumEntries as usize {
        let row = unsafe { &*first_row.add(index) };
        received = received.saturating_add(row.InOctets);
        sent = sent.saturating_add(row.OutOctets);
    }
    unsafe { FreeMibTable(table as *const c_void) };
    (received, sent)
}

#[cfg(not(windows))]
fn network_totals() -> (u64, u64) {
    (0, 0)
}

#[cfg(windows)]
fn start_tracker(state: SharedState, path: SharedPath) {
    use chrono::{Local, Timelike};
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    thread::spawn(move || {
        let mut previous_window = String::new();
        let mut previous_cursor = POINT { x: 0, y: 0 };
        let mut previous_keys = [false; 255];
        let mut previous_left = false;
        let mut previous_right = false;
        let mut last_click: Option<Instant> = None;
        let mut last_second = Instant::now();
        let mut last_save = Instant::now();
        let mut last_minute = Local::now().minute();
        let mut previous_network = network_totals();

        loop {
            thread::sleep(Duration::from_millis(50));

            let enabled = state.lock().map(|value| value.enabled).unwrap_or(false);
            if !enabled {
                continue;
            }

            let mut cursor = POINT { x: 0, y: 0 };
            let cursor_ok = unsafe { GetCursorPos(&mut cursor) }.is_ok();
            let left_down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
            let right_down = unsafe { GetAsyncKeyState(VK_RBUTTON.0 as i32) } < 0;
            let click_started = (left_down && !previous_left) || (right_down && !previous_right);
            previous_left = left_down;
            previous_right = right_down;

            let mut key_presses = 0u64;
            for key in 0..255 {
                let down = unsafe { GetAsyncKeyState(key as i32) } < 0;
                if down && !previous_keys[key] {
                    key_presses += 1;
                }
                previous_keys[key] = down;
            }

            if let Ok(mut current) = state.lock() {
                current.keyboard_actions += key_presses;

                if cursor_ok {
                    let dx = (cursor.x - previous_cursor.x) as f64;
                    let dy = (cursor.y - previous_cursor.y) as f64;
                    let distance = (dx * dx + dy * dy).sqrt();
                    if distance > 0.0 {
                        current.mouse_actions += 1;
                        current.mouse_distance_px += distance;
                    }
                    previous_cursor = cursor;
                }

                if click_started {
                    current.mouse_clicks += 1;
                    if let Some(previous) = last_click {
                        let interval = previous.elapsed().as_millis() as u64;
                        current.avg_click_interval_ms = if current.avg_click_interval_ms == 0 {
                            interval
                        } else {
                            (current.avg_click_interval_ms * 4 + interval) / 5
                        };
                    }
                    last_click = Some(Instant::now());
                }
            }

            if last_second.elapsed() >= Duration::from_secs(1) {
                let now = Local::now();
                let window = active_window();
                let switched = window != previous_window && !window.is_empty();
                if let Ok(mut current) = state.lock() {
                    current.focus_seconds += 1;
                    current.hourly_focus[now.hour() as usize] += 1;
                    current.active_window = window.clone();

                    if switched {
                        current.app_activation_count += 1;
                        current.timeline.push(TimelineEvent {
                            at: now.to_rfc3339(),
                            app: window.clone(),
                            title: window.clone(),
                            kind: "app_switch".into(),
                        });
                        if current.timeline.len() > 10_000 {
                            current.timeline.remove(0);
                        }
                        previous_window = window.clone();
                    }

                    if let Some(app) = current.apps.iter_mut().find(|app| app.app == window) {
                        app.seconds += 1;
                        app.title = window.clone();
                        if switched {
                            app.activations += 1;
                        }
                    } else if !window.is_empty() {
                        current.apps.push(AppActivity {
                            app: window.clone(),
                            title: window,
                            seconds: 1,
                            activations: 1,
                        });
                    }
                }
                last_second = Instant::now();
            }

            let now = Local::now();
            if now.minute() != last_minute {
                let network = network_totals();
                let received_delta = network.0.saturating_sub(previous_network.0);
                let sent_delta = network.1.saturating_sub(previous_network.1);
                previous_network = network;
                let app_count = visible_app_count();

                if let Ok(mut current) = state.lock() {
                    current.active_app_count = app_count;
                    current.network_rx_bytes = current.network_rx_bytes.saturating_add(received_delta);
                    current.network_tx_bytes = current.network_tx_bytes.saturating_add(sent_delta);
                    let snapshot = MinuteSnapshot {
                        at: now.to_rfc3339(),
                        active_app_count: current.active_app_count,
                        focus_seconds: current.focus_seconds,
                        keyboard_actions: current.keyboard_actions,
                        mouse_actions: current.mouse_actions,
                        mouse_distance_px: current.mouse_distance_px,
                        network_rx_bytes: current.network_rx_bytes,
                        network_tx_bytes: current.network_tx_bytes,
                    };
                    current.minute_snapshots.push(snapshot);
                    if current.minute_snapshots.len() > 43_200 {
                        current.minute_snapshots.remove(0);
                    }
                }
                last_minute = now.minute();
            }

            if last_save.elapsed() >= Duration::from_secs(1) {
                if let (Ok(current), Ok(file)) = (state.lock(), storage_path(&path)) {
                    let _ = save(&current, &file);
                }
                last_save = Instant::now();
            }
        }
    });
}

#[cfg(not(windows))]
fn start_tracker(_state: SharedState, _path: SharedPath) {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = Arc::new(Mutex::new(TrackingState::default()));
    let path = Arc::new(Mutex::new(PathBuf::new()));
    let setup_state = state.clone();
    let setup_path = path.clone();

    tauri::Builder::default()
        .setup(move |app| {
            let file = app
                .path()
                .app_data_dir()
                .map_err(|e| e.to_string())?
                .join("activity.json");
            if let Ok(mut current) = setup_state.lock() {
                *current = load(&file);
            }
            if let Ok(mut current_path) = setup_path.lock() {
                *current_path = file;
            }
            start_tracker(setup_state.clone(), setup_path.clone());
            Ok(())
        })
        .manage(state)
        .manage(path)
        .invoke_handler(tauri::generate_handler![
            set_tracking,
            get_tracking_state,
            clear_all_data
        ])
        .run(tauri::generate_context!())
        .expect("error while running FlowLens");
}
