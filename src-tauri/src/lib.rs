use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use tauri::Manager;

const IDLE_THRESHOLD_SECS: u64 = 5 * 60;
const MAX_TIMELINE: usize = 10_000;
const MAX_MINUTE_SNAPSHOTS: usize = 43_200;

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct AppActivity { app: String, title: String, seconds: u64, activations: u64 }

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct TimelineEvent { at: String, app: String, title: String, kind: String }

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
    idle_seconds: u64,
    context_switches: u64,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct WorkSession {
    started_at: String,
    ended_at: Option<String>,
    active_seconds: u64,
    idle_seconds: u64,
    switch_count: u64,
    app_count: u64,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct DailyFeatureVector {
    schema_version: u32,
    date: String,
    focus_minutes: f64,
    idle_minutes: f64,
    active_ratio: f64,
    app_count: u64,
    context_switches: u64,
    switches_per_active_hour: f64,
    average_session_minutes: f64,
    keyboard_events_per_active_minute: f64,
    mouse_distance_per_active_minute: f64,
    click_interval_variance_ms: f64,
    network_bytes_per_active_minute: f64,
    peak_focus_hour: u8,
    work_start_hour: Option<u8>,
    work_end_hour: Option<u8>,
    app_time_share: Vec<f64>,
    local_only: bool,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct AdvancedCollectionState {
    notification_requested: bool,
    notification_access: String,
    per_app_network_requested: bool,
    per_app_network_status: String,
    helper_last_error: String,
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
    click_interval_count: u64,
    click_interval_mean_ms: f64,
    click_interval_m2: f64,
    network_rx_bytes: u64,
    network_tx_bytes: u64,
    notification_count: u64,
    idle_seconds: u64,
    context_switches: u64,
    sessions: Vec<WorkSession>,
    current_session_started_at: Option<String>,
    current_session_active_seconds: u64,
    current_session_idle_seconds: u64,
    current_session_switches: u64,
    current_session_apps: Vec<String>,
    last_input_at: Option<String>,
    last_input_age_seconds: u64,
    resume_latency_seconds: u64,
    daily_feature: DailyFeatureVector,
    advanced: AdvancedCollectionState,
}

impl Default for TrackingState {
    fn default() -> Self {
        Self {
            enabled: true, active_window: String::new(), keyboard_actions: 0, mouse_actions: 0,
            app_activation_count: 0, focus_seconds: 0, hourly_focus: [0; 24], apps: Vec::new(),
            timeline: Vec::new(), minute_snapshots: Vec::new(), active_app_count: 0,
            mouse_distance_px: 0.0, mouse_clicks: 0, avg_click_interval_ms: 0,
            click_interval_count: 0, click_interval_mean_ms: 0.0, click_interval_m2: 0.0,
            network_rx_bytes: 0, network_tx_bytes: 0, notification_count: 0, idle_seconds: 0,
            context_switches: 0, sessions: Vec::new(), current_session_started_at: None,
            current_session_active_seconds: 0, current_session_idle_seconds: 0,
            current_session_switches: 0, current_session_apps: Vec::new(),
            last_input_at: None, last_input_age_seconds: 0, resume_latency_seconds: 0,
            daily_feature: DailyFeatureVector { schema_version: 1, local_only: true, ..Default::default() },
            advanced: AdvancedCollectionState { notification_access: "not_requested".into(), per_app_network_status: "not_requested".into(), ..Default::default() },
        }
    }
}

type SharedState = Arc<Mutex<TrackingState>>;
type SharedPath = Arc<Mutex<PathBuf>>;

fn save(state: &TrackingState, path: &PathBuf) -> Result<(), String> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    fs::rename(temp, path).map_err(|e| e.to_string())
}
fn load(path: &PathBuf) -> TrackingState { fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default() }
fn storage_path(path: &SharedPath) -> Result<PathBuf, String> { path.lock().map(|value| value.clone()).map_err(|_| "storage path unavailable".into()) }

fn record_event(state: &mut TrackingState, at: String, app: String, title: String, kind: &str) {
    state.timeline.push(TimelineEvent { at, app, title, kind: kind.into() });
    if state.timeline.len() > MAX_TIMELINE { state.timeline.remove(0); }
}

fn close_current_session(state: &mut TrackingState, ended_at: String) {
    if let Some(started_at) = state.current_session_started_at.take() {
        state.sessions.push(WorkSession {
            started_at, ended_at: Some(ended_at), active_seconds: state.current_session_active_seconds,
            idle_seconds: state.current_session_idle_seconds, switch_count: state.current_session_switches,
            app_count: state.current_session_apps.len() as u64,
        });
        if state.sessions.len() > 500 { state.sessions.remove(0); }
    }
    state.current_session_active_seconds = 0;
    state.current_session_idle_seconds = 0;
    state.current_session_switches = 0;
    state.current_session_apps.clear();
}

fn update_daily_feature(state: &mut TrackingState, now: chrono::DateTime<chrono::Local>) {
    use chrono::Timelike;
    let focus_minutes = state.focus_seconds as f64 / 60.0;
    let idle_minutes = state.idle_seconds as f64 / 60.0;
    let total_minutes = focus_minutes + idle_minutes;
    let session_seconds: u64 = state.sessions.iter().map(|s| s.active_seconds).sum::<u64>() + state.current_session_active_seconds;
    let session_count = state.sessions.len() as f64 + if state.current_session_started_at.is_some() { 1.0 } else { 0.0 };
    let max_hour = state.hourly_focus.iter().enumerate().max_by_key(|(_, seconds)| **seconds).map(|(hour, _)| hour as u8).unwrap_or(0);
    let work_start = state.hourly_focus.iter().position(|value| *value > 0).map(|hour| hour as u8);
    let work_end = state.hourly_focus.iter().rposition(|value| *value > 0).map(|hour| hour as u8);
    let total_app_seconds = state.apps.iter().map(|app| app.seconds).sum::<u64>().max(1) as f64;
    let mut shares: Vec<f64> = state.apps.iter().map(|app| app.seconds as f64 / total_app_seconds).collect();
    shares.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    shares.truncate(12);
    state.daily_feature = DailyFeatureVector {
        schema_version: 1,
        date: now.date_naive().to_string(),
        focus_minutes,
        idle_minutes,
        active_ratio: if total_minutes > 0.0 { focus_minutes / total_minutes } else { 0.0 },
        app_count: state.apps.len() as u64,
        context_switches: state.context_switches,
        switches_per_active_hour: if focus_minutes > 0.0 { state.context_switches as f64 / (focus_minutes / 60.0) } else { 0.0 },
        average_session_minutes: if session_count > 0.0 { session_seconds as f64 / 60.0 / session_count } else { 0.0 },
        keyboard_events_per_active_minute: if focus_minutes > 0.0 { state.keyboard_actions as f64 / focus_minutes } else { 0.0 },
        mouse_distance_per_active_minute: if focus_minutes > 0.0 { state.mouse_distance_px / focus_minutes } else { 0.0 },
        click_interval_variance_ms: if state.click_interval_count > 1 { state.click_interval_m2 / (state.click_interval_count - 1) as f64 } else { 0.0 },
        network_bytes_per_active_minute: if focus_minutes > 0.0 { (state.network_rx_bytes + state.network_tx_bytes) as f64 / focus_minutes } else { 0.0 },
        peak_focus_hour: max_hour,
        work_start_hour: work_start,
        work_end_hour: work_end,
        app_time_share: shares,
        local_only: true,
    };
}

#[tauri::command]
fn set_tracking(enabled: bool, state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<TrackingState, String> {
    let mut current = state.lock().map_err(|_| "state unavailable")?;
    current.enabled = enabled;
    save(&current, &storage_path(&path)?)?;
    Ok(current.clone())
}
#[tauri::command]
fn get_tracking_state(state: tauri::State<'_, SharedState>) -> Result<TrackingState, String> { state.lock().map(|value| value.clone()).map_err(|_| "state unavailable".into()) }
#[tauri::command]
fn clear_all_data(state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<(), String> {
    let mut current = state.lock().map_err(|_| "state unavailable")?;
    let enabled = current.enabled; *current = TrackingState { enabled, ..Default::default() };
    let file = storage_path(&path)?; if file.exists() { fs::remove_file(file).map_err(|e| e.to_string())?; }
    Ok(())
}
#[tauri::command]
fn get_daily_feature_vector(state: tauri::State<'_, SharedState>) -> Result<DailyFeatureVector, String> { state.lock().map(|value| value.daily_feature.clone()).map_err(|_| "state unavailable".into()) }
#[tauri::command]
fn request_notification_access(state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<AdvancedCollectionState, String> {
    let mut current = state.lock().map_err(|_| "state unavailable")?;
    current.advanced.notification_requested = true;
    current.advanced.notification_access = "requires_msix_user_notification_listener_consent".into();
    current.advanced.helper_last_error = "알림 수집은 관리자 권한이 아니라 MSIX 패키지 identity와 Windows 알림 접근 동의가 필요합니다.".into();
    save(&current, &storage_path(&path)?)?; Ok(current.advanced.clone())
}
#[tauri::command]
fn request_per_app_network_collection(state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<AdvancedCollectionState, String> {
    let mut current = state.lock().map_err(|_| "state unavailable")?;
    current.advanced.per_app_network_requested = true;
    current.advanced.per_app_network_status = "admin_required_etw_helper_not_installed".into();
    current.advanced.helper_last_error = "앱별 네트워크 바이트는 시스템 ETW 세션을 사용하므로 관리자 권한으로 설치되는 선택적 수집기가 필요합니다.".into();
    save(&current, &storage_path(&path)?)?; Ok(current.advanced.clone())
}

#[cfg(windows)]
fn active_window() -> String {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
    let hwnd = unsafe { GetForegroundWindow() }; if hwnd == HWND::default() { return String::new(); }
    let mut buffer = [0u16; 512]; let length = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..length as usize])
}
#[cfg(windows)]
unsafe extern "system" fn count_visible_window(hwnd: windows::Win32::Foundation::HWND, lparam: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::BOOL {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, IsWindowVisible};
    if IsWindowVisible(hwnd).as_bool() && GetWindowTextLengthW(hwnd) > 0 { let count = &mut *(lparam.0 as *mut u64); *count += 1; }
    BOOL(1)
}
#[cfg(windows)]
fn visible_app_count() -> u64 {
    use windows::Win32::Foundation::LPARAM; use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
    let mut count = 0u64; let _ = unsafe { EnumWindows(Some(count_visible_window), LPARAM((&mut count as *mut u64) as isize)) }; count
}
#[cfg(windows)]
fn network_totals() -> (u64, u64) {
    use std::ffi::c_void;
    use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut(); let result = unsafe { GetIfTable2(&mut table) };
    if result.0 != 0 || table.is_null() { return (0, 0); }
    let table_ref = unsafe { &*table }; let first_row = table_ref.Table.as_ptr(); let mut received = 0u64; let mut sent = 0u64;
    for index in 0..table_ref.NumEntries as usize { let row = unsafe { &*first_row.add(index) }; received = received.saturating_add(row.InOctets); sent = sent.saturating_add(row.OutOctets); }
    unsafe { FreeMibTable(table as *const c_void) }; (received, sent)
}
#[cfg(not(windows))]
fn network_totals() -> (u64, u64) { (0, 0) }

#[cfg(windows)]
fn start_tracker(state: SharedState, path: SharedPath) {
    use chrono::{Local, Timelike};
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    thread::spawn(move || {
        let mut previous_window = String::new(); let mut previous_cursor = POINT { x: 0, y: 0 };
        let mut previous_keys = [false; 255]; let mut previous_left = false; let mut previous_right = false;
        let mut last_click: Option<Instant> = None; let mut last_second = Instant::now(); let mut last_save = Instant::now();
        let mut last_minute = Local::now().minute(); let mut previous_network = network_totals(); let mut last_input = Instant::now();
        loop {
            thread::sleep(Duration::from_millis(50));
            if !state.lock().map(|value| value.enabled).unwrap_or(false) { continue; }
            let mut cursor = POINT { x: 0, y: 0 }; let cursor_ok = unsafe { GetCursorPos(&mut cursor) }.is_ok();
            let left_down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0; let right_down = unsafe { GetAsyncKeyState(VK_RBUTTON.0 as i32) } < 0;
            let click_started = (left_down && !previous_left) || (right_down && !previous_right); previous_left = left_down; previous_right = right_down;
            let mut key_presses = 0u64;
            for key in 0..255 { let down = unsafe { GetAsyncKeyState(key as i32) } < 0; if down && !previous_keys[key] { key_presses += 1; } previous_keys[key] = down; }
            let mut input_seen = key_presses > 0 || click_started;
            if let Ok(mut current) = state.lock() {
                current.keyboard_actions += key_presses;
                if cursor_ok { let dx = (cursor.x - previous_cursor.x) as f64; let dy = (cursor.y - previous_cursor.y) as f64; let distance = (dx * dx + dy * dy).sqrt(); if distance > 0.0 { current.mouse_actions += 1; current.mouse_distance_px += distance; input_seen = true; } previous_cursor = cursor; }
                if click_started { current.mouse_clicks += 1; if let Some(previous) = last_click { let interval = previous.elapsed().as_millis() as u64; current.click_interval_count += 1; let delta = interval as f64 - current.click_interval_mean_ms; current.click_interval_mean_ms += delta / current.click_interval_count as f64; current.click_interval_m2 += delta * (interval as f64 - current.click_interval_mean_ms); current.avg_click_interval_ms = current.click_interval_mean_ms.round() as u64; } last_click = Some(Instant::now()); }
                if input_seen { last_input = Instant::now(); current.last_input_at = Some(Local::now().to_rfc3339()); current.last_input_age_seconds = 0; } else { current.last_input_age_seconds = last_input.elapsed().as_secs(); }
            }
            if last_second.elapsed() >= Duration::from_secs(1) {
                let now = Local::now(); let window = active_window(); let switched = window != previous_window && !window.is_empty(); let idle = last_input.elapsed().as_secs() >= IDLE_THRESHOLD_SECS;
                if let Ok(mut current) = state.lock() {
                    if idle { current.idle_seconds += 1; current.current_session_idle_seconds += 1; } else { current.focus_seconds += 1; current.current_session_active_seconds += 1; current.hourly_focus[now.hour() as usize] += 1; }
                    current.active_window = window.clone();
                    if !idle && current.current_session_started_at.is_none() { current.resume_latency_seconds = current.last_input_age_seconds; current.current_session_started_at = Some(now.to_rfc3339()); record_event(&mut current, now.to_rfc3339(), window.clone(), window.clone(), "session_start"); }
                    if idle && current.current_session_started_at.is_some() { record_event(&mut current, now.to_rfc3339(), window.clone(), window.clone(), "session_end_idle"); close_current_session(&mut current, now.to_rfc3339()); }
                    if switched { current.app_activation_count += 1; current.context_switches += 1; current.current_session_switches += 1; if !current.current_session_apps.iter().any(|app| app == &window) { current.current_session_apps.push(window.clone()); } record_event(&mut current, now.to_rfc3339(), window.clone(), window.clone(), "app_switch"); previous_window = window.clone(); }
                    if !idle { if let Some(app) = current.apps.iter_mut().find(|app| app.app == window) { app.seconds += 1; app.title = window.clone(); if switched { app.activations += 1; } } else if !window.is_empty() { current.apps.push(AppActivity { app: window.clone(), title: window, seconds: 1, activations: 1 }); } }
                    update_daily_feature(&mut current, now);
                }
                last_second = Instant::now();
            }
            let now = Local::now();
            if now.minute() != last_minute {
                let network = network_totals(); let received_delta = network.0.saturating_sub(previous_network.0); let sent_delta = network.1.saturating_sub(previous_network.1); previous_network = network;
                if let Ok(mut current) = state.lock() {
                    current.active_app_count = visible_app_count(); current.network_rx_bytes = current.network_rx_bytes.saturating_add(received_delta); current.network_tx_bytes = current.network_tx_bytes.saturating_add(sent_delta);
                    current.minute_snapshots.push(MinuteSnapshot { at: now.to_rfc3339(), active_app_count: current.active_app_count, focus_seconds: current.focus_seconds, keyboard_actions: current.keyboard_actions, mouse_actions: current.mouse_actions, mouse_distance_px: current.mouse_distance_px, network_rx_bytes: current.network_rx_bytes, network_tx_bytes: current.network_tx_bytes, idle_seconds: current.idle_seconds, context_switches: current.context_switches });
                    if current.minute_snapshots.len() > MAX_MINUTE_SNAPSHOTS { current.minute_snapshots.remove(0); }
                }
                last_minute = now.minute();
            }
            if last_save.elapsed() >= Duration::from_secs(1) { if let (Ok(current), Ok(file)) = (state.lock(), storage_path(&path)) { let _ = save(&current, &file); } last_save = Instant::now(); }
        }
    });
}
#[cfg(not(windows))]
fn start_tracker(_state: SharedState, _path: SharedPath) {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = Arc::new(Mutex::new(TrackingState::default())); let path = Arc::new(Mutex::new(PathBuf::new())); let setup_state = state.clone(); let setup_path = path.clone();
    tauri::Builder::default().setup(move |app| {
        let file = app.path().app_data_dir().map_err(|e| e.to_string())?.join("activity.json");
        if let Ok(mut current) = setup_state.lock() { *current = load(&file); }
        if let Ok(mut current_path) = setup_path.lock() { *current_path = file; }
        start_tracker(setup_state.clone(), setup_path.clone()); Ok(())
    }).manage(state).manage(path).invoke_handler(tauri::generate_handler![set_tracking, get_tracking_state, clear_all_data, get_daily_feature_vector, request_notification_access, request_per_app_network_collection]).run(tauri::generate_context!()).expect("error while running FlowLens");
}
