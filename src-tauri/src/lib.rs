use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
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
const MAX_FLOW_MINUTES: usize = 1_440;
const MAX_SESSIONS: usize = 500;
const FOCUS_BUCKET_MINUTES: u8 = 5;
const MAX_EMBEDDING_HISTORY: usize = 365;
const EMBEDDING_DIMENSIONS: usize = 24;
const FOREGROUND_SAMPLE_MS: u64 = 200;

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct AppActivity { app: String, title: String, seconds: u64, active_millis: u64, activations: u64 }

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
struct FlowMinute {
    start_at: String,
    observed_seconds: u16,
    focus_seconds: u16,
    idle_seconds: u16,
    switch_count: u16,
    keyboard_actions: u32,
    mouse_actions: u32,
    mouse_distance_px: f32,
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
    end_reason: Option<String>,
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
    session_count: u64,
    keyboard_events_per_active_minute: f64,
    mouse_distance_per_active_minute: f64,
    mouse_clicks_per_active_minute: f64,
    click_interval_variance_ms: f64,
    network_bytes_per_active_minute: f64,
    peak_focus_hour: u8,
    work_start_hour: Option<u8>,
    work_end_hour: Option<u8>,
    work_span_hours: f64,
    app_time_share: Vec<f64>,
    local_only: bool,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct EmbeddingRecord {
    schema_version: u32,
    date: String,
    created_at: String,
    dimensions: usize,
    embedding: Vec<f64>,
    data_confidence: f64,
    feature: DailyFeatureVector,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct SimilarDay {
    date: String,
    similarity: f64,
    focus_delta_minutes: f64,
    switch_delta: f64,
    active_ratio_delta: f64,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct PatternInsight {
    level: String,
    title: String,
    detail: String,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct EmbeddingAnalysis {
    enabled: bool,
    local_only: bool,
    dimensions: usize,
    history_days: usize,
    current: Option<EmbeddingRecord>,
    baseline_similarity: Option<f64>,
    similar_days: Vec<SimilarDay>,
    insights: Vec<PatternInsight>,
    notice: String,
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

#[derive(Clone, Serialize)]
struct FocusExperienceView {
    schema_version: u32,
    date: String,
    generated_at: String,
    is_live: bool,
    ribbon: FocusFlowView,
    sessions: SessionReviewView,
}

#[derive(Clone, Serialize)]
struct FocusFlowView {
    bucket_minutes: u8,
    day_start_at: String,
    day_end_at: String,
    buckets: Vec<FocusFlowBucketView>,
    summary: FocusFlowSummary,
}

#[derive(Clone, Serialize)]
struct FocusFlowBucketView {
    start_at: String,
    end_at: String,
    state: String,
    observed_seconds: u16,
    focus_seconds: u16,
    idle_seconds: u16,
    switch_count: u16,
    input_actions: u32,
    intensity: f32,
}

#[derive(Clone, Serialize)]
struct FocusFlowSummary {
    focus_seconds: u64,
    idle_seconds: u64,
    longest_focused_span_seconds: u64,
    highest_switch_bucket_start_at: Option<String>,
    highest_switch_count: u16,
}

#[derive(Clone, Serialize)]
struct SessionReviewView {
    sessions: Vec<SessionCardView>,
    summary: SessionSummary,
}

#[derive(Clone, Serialize)]
struct SessionCardView {
    id: String,
    started_at: String,
    ended_at: Option<String>,
    is_live: bool,
    status: String,
    active_seconds: u64,
    app_count: u64,
    switch_count: u64,
    switches_per_active_hour: f32,
    end_reason: Option<String>,
}

#[derive(Clone, Serialize)]
struct SessionSummary {
    completed_count: u64,
    active_seconds: u64,
    longest_session_seconds: u64,
    average_session_seconds: u64,
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
    flow_minutes: Vec<FlowMinute>,
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
    collection_day: String,
    daily_feature: DailyFeatureVector,
    embedding_enabled: bool,
    embedding_history: Vec<EmbeddingRecord>,
    advanced: AdvancedCollectionState,
    tracker_sample_interval_ms: u64,
    unresolved_window_count: u64,
}

impl Default for TrackingState {
    fn default() -> Self {
        Self {
            enabled: true, active_window: String::new(), keyboard_actions: 0, mouse_actions: 0,
            app_activation_count: 0, focus_seconds: 0, hourly_focus: [0; 24], apps: Vec::new(),
            timeline: Vec::new(), minute_snapshots: Vec::new(), flow_minutes: Vec::new(), active_app_count: 0,
            mouse_distance_px: 0.0, mouse_clicks: 0, avg_click_interval_ms: 0,
            click_interval_count: 0, click_interval_mean_ms: 0.0, click_interval_m2: 0.0,
            network_rx_bytes: 0, network_tx_bytes: 0, notification_count: 0, idle_seconds: 0,
            context_switches: 0, sessions: Vec::new(), current_session_started_at: None,
            current_session_active_seconds: 0, current_session_idle_seconds: 0,
            current_session_switches: 0, current_session_apps: Vec::new(),
            last_input_at: None, last_input_age_seconds: 0, resume_latency_seconds: 0,
            collection_day: String::new(),
            daily_feature: DailyFeatureVector { schema_version: 2, local_only: true, ..Default::default() },
            embedding_enabled: false, embedding_history: Vec::new(),
            advanced: AdvancedCollectionState { notification_access: "not_requested".into(), per_app_network_status: "not_requested".into(), ..Default::default() },
            tracker_sample_interval_ms: FOREGROUND_SAMPLE_MS, unresolved_window_count: 0,
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


fn ensure_app<'a>(state: &'a mut TrackingState, app_name: &str, title: &str) -> &'a mut AppActivity {
    if let Some(index) = state.apps.iter().position(|item| item.app == app_name) {
        let item = &mut state.apps[index];
        if item.active_millis < item.seconds.saturating_mul(1000) { item.active_millis = item.seconds.saturating_mul(1000); }
        if !title.is_empty() && title != "제목 없음" { item.title = title.to_string(); }
        return item;
    }
    state.apps.push(AppActivity { app: app_name.to_string(), title: title.to_string(), seconds: 0, active_millis: 0, activations: 0 });
    state.apps.last_mut().expect("app activity inserted")
}

fn record_app_focus(state: &mut TrackingState, app_name: &str, title: &str, elapsed_ms: u64, activated: bool) {
    let item = ensure_app(state, app_name, title);
    if activated { item.activations = item.activations.saturating_add(1); }
    item.active_millis = item.active_millis.saturating_add(elapsed_ms);
    item.seconds = item.active_millis / 1000;
}

fn flow_minute_key(now: chrono::DateTime<chrono::Local>) -> String {
    now.format("%Y-%m-%dT%H:%M:00%:z").to_string()
}

fn flow_minute_for<'a>(state: &'a mut TrackingState, now: chrono::DateTime<chrono::Local>) -> &'a mut FlowMinute {
    let key = flow_minute_key(now);
    if let Some(index) = state.flow_minutes.iter().position(|item| item.start_at == key) {
        return &mut state.flow_minutes[index];
    }
    state.flow_minutes.push(FlowMinute { start_at: key, ..Default::default() });
    if state.flow_minutes.len() > MAX_FLOW_MINUTES { state.flow_minutes.remove(0); }
    state.flow_minutes.last_mut().expect("flow minute inserted")
}

fn record_flow_input(
    state: &mut TrackingState,
    now: chrono::DateTime<chrono::Local>,
    keyboard_actions: u64,
    mouse_actions: u32,
    mouse_distance_px: f32,
) {
    if keyboard_actions == 0 && mouse_actions == 0 && mouse_distance_px <= 0.0 { return; }
    let minute = flow_minute_for(state, now);
    minute.keyboard_actions = minute.keyboard_actions.saturating_add(keyboard_actions.min(u32::MAX as u64) as u32);
    minute.mouse_actions = minute.mouse_actions.saturating_add(mouse_actions);
    minute.mouse_distance_px += mouse_distance_px.max(0.0);
}

#[derive(Clone, Default)]
struct FlowBucketAccumulator {
    observed_seconds: u32,
    focus_seconds: u32,
    idle_seconds: u32,
    switch_count: u32,
    input_actions: u32,
}

fn flow_bucket_timestamp(date: &str, minute_of_day: usize) -> String {
    let day_offset = minute_of_day / 1_440;
    let minute = minute_of_day % 1_440;
    let day = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .unwrap_or_else(|_| chrono::Local::now().date_naive())
        + chrono::Duration::days(day_offset as i64);
    format!("{}T{:02}:{:02}:00{}", day.format("%Y-%m-%d"), minute / 60, minute % 60, chrono::Local::now().format("%:z"))
}

fn flow_minute_slot(item: &FlowMinute, date: &str, bucket_minutes: u8) -> Option<usize> {
    use chrono::Timelike;
    if !item.start_at.starts_with(date) { return None; }
    let timestamp = chrono::DateTime::parse_from_rfc3339(&item.start_at).ok()?;
    let minute_of_day = timestamp.hour() as usize * 60 + timestamp.minute() as usize;
    Some(minute_of_day / bucket_minutes as usize)
}

fn flow_state_and_intensity(values: &FlowBucketAccumulator) -> (String, f32) {
    if values.observed_seconds < 30 { return ("unobserved".into(), 0.0); }
    let observed = values.observed_seconds.max(1) as f32;
    let focus_ratio = values.focus_seconds as f32 / observed;
    let idle_ratio = values.idle_seconds as f32 / observed;
    let switches_per_minute = values.switch_count as f32 / (observed / 60.0).max(1.0);
    let input_density = ((1.0 + values.input_actions as f32).ln() / (121.0_f32).ln()).clamp(0.0, 1.0);
    let switch_penalty = (values.switch_count as f32 / 6.0).clamp(0.0, 1.0);
    let intensity = (0.65 * focus_ratio + 0.25 * input_density + 0.10 * (1.0 - switch_penalty)).clamp(0.0, 1.0);
    if idle_ratio >= 0.60 { ("idle".into(), intensity.min(0.25)) }
    else if values.switch_count >= 4 || switches_per_minute >= 0.80 { ("switching".into(), intensity) }
    else if focus_ratio >= 0.75 && values.switch_count <= 2 { ("focused".into(), intensity) }
    else { ("mixed".into(), intensity) }
}

fn session_status(active_seconds: u64, switch_count: u64) -> String {
    let switches_per_hour = if active_seconds > 0 { switch_count as f64 / (active_seconds as f64 / 3600.0) } else { 0.0 };
    if active_seconds < 10 * 60 { "brief".into() }
    else if active_seconds >= 25 * 60 && switches_per_hour < 6.0 { "focused".into() }
    else if active_seconds >= 15 * 60 && switches_per_hour < 12.0 { "steady".into() }
    else { "fragmented".into() }
}

fn session_card(
    id: String,
    started_at: String,
    ended_at: Option<String>,
    is_live: bool,
    active_seconds: u64,
    app_count: u64,
    switch_count: u64,
    end_reason: Option<String>,
) -> SessionCardView {
    let switches_per_active_hour = if active_seconds > 0 {
        switch_count as f64 / (active_seconds as f64 / 3600.0)
    } else { 0.0 };
    SessionCardView {
        id,
        started_at,
        ended_at,
        is_live,
        status: session_status(active_seconds, switch_count),
        active_seconds,
        app_count,
        switch_count,
        switches_per_active_hour: switches_per_active_hour as f32,
        end_reason,
    }
}

fn build_focus_experience_view(state: &TrackingState, date: &str, bucket_minutes: u8) -> FocusExperienceView {
    let bucket_count = 1_440 / bucket_minutes as usize;
    let mut aggregates = vec![FlowBucketAccumulator::default(); bucket_count];
    for minute in &state.flow_minutes {
        if let Some(slot) = flow_minute_slot(minute, date, bucket_minutes) {
            if let Some(values) = aggregates.get_mut(slot) {
                values.observed_seconds = values.observed_seconds.saturating_add(minute.observed_seconds as u32);
                values.focus_seconds = values.focus_seconds.saturating_add(minute.focus_seconds as u32);
                values.idle_seconds = values.idle_seconds.saturating_add(minute.idle_seconds as u32);
                values.switch_count = values.switch_count.saturating_add(minute.switch_count as u32);
                values.input_actions = values.input_actions.saturating_add(minute.keyboard_actions.saturating_add(minute.mouse_actions));
            }
        }
    }

    let mut focus_seconds = 0u64;
    let mut idle_seconds = 0u64;
    let mut longest_focused_span_seconds = 0u64;
    let mut current_focused_span_seconds = 0u64;
    let mut highest_switch_count = 0u16;
    let mut highest_switch_bucket_start_at = None;
    let mut buckets = Vec::with_capacity(bucket_count);

    for (index, values) in aggregates.iter().enumerate() {
        let (state_name, intensity) = flow_state_and_intensity(values);
        let start_at = flow_bucket_timestamp(date, index * bucket_minutes as usize);
        let end_at = flow_bucket_timestamp(date, (index + 1) * bucket_minutes as usize);
        let observed = values.observed_seconds.min(u16::MAX as u32) as u16;
        let focus = values.focus_seconds.min(u16::MAX as u32) as u16;
        let idle = values.idle_seconds.min(u16::MAX as u32) as u16;
        let switches = values.switch_count.min(u16::MAX as u32) as u16;
        focus_seconds = focus_seconds.saturating_add(values.focus_seconds as u64);
        idle_seconds = idle_seconds.saturating_add(values.idle_seconds as u64);
        if state_name == "focused" {
            current_focused_span_seconds = current_focused_span_seconds.saturating_add(values.focus_seconds as u64);
            longest_focused_span_seconds = longest_focused_span_seconds.max(current_focused_span_seconds);
        } else { current_focused_span_seconds = 0; }
        if switches > highest_switch_count {
            highest_switch_count = switches;
            highest_switch_bucket_start_at = Some(start_at.clone());
        }
        buckets.push(FocusFlowBucketView {
            start_at,
            end_at,
            state: state_name,
            observed_seconds: observed,
            focus_seconds: focus,
            idle_seconds: idle,
            switch_count: switches,
            input_actions: values.input_actions,
            intensity,
        });
    }

    let mut cards: Vec<SessionCardView> = state.sessions.iter().enumerate()
        .filter(|(_, session)| session.started_at.starts_with(date))
        .map(|(index, session)| session_card(
            format!("{}-{index}", session.started_at),
            session.started_at.clone(),
            session.ended_at.clone(),
            false,
            session.active_seconds,
            session.app_count,
            session.switch_count,
            session.end_reason.clone(),
        )).collect();
    if let Some(started_at) = state.current_session_started_at.as_ref().filter(|value| value.starts_with(date)) {
        cards.push(session_card(
            format!("live-{started_at}"),
            started_at.clone(),
            None,
            state.enabled,
            state.current_session_active_seconds,
            state.current_session_apps.len() as u64,
            state.current_session_switches,
            None,
        ));
    }
    cards.sort_by(|left, right| right.started_at.cmp(&left.started_at));
    let completed_count = cards.iter().filter(|item| !item.is_live).count() as u64;
    let active_seconds = cards.iter().map(|item| item.active_seconds).sum::<u64>();
    let longest_session_seconds = cards.iter().map(|item| item.active_seconds).max().unwrap_or(0);
    let average_session_seconds = if cards.is_empty() { 0 } else { active_seconds / cards.len() as u64 };
    let is_live = state.enabled && cards.iter().any(|item| item.is_live);

    FocusExperienceView {
        schema_version: 1,
        date: date.into(),
        generated_at: chrono::Local::now().to_rfc3339(),
        is_live,
        ribbon: FocusFlowView {
            bucket_minutes,
            day_start_at: flow_bucket_timestamp(date, 0),
            day_end_at: flow_bucket_timestamp(date, 1_440),
            buckets,
            summary: FocusFlowSummary { focus_seconds, idle_seconds, longest_focused_span_seconds, highest_switch_bucket_start_at, highest_switch_count },
        },
        sessions: SessionReviewView {
            sessions: cards,
            summary: SessionSummary { completed_count, active_seconds, longest_session_seconds, average_session_seconds },
        },
    }
}

fn record_event(state: &mut TrackingState, at: String, app: String, title: String, kind: &str) {
    state.timeline.push(TimelineEvent { at, app, title, kind: kind.into() });
    if state.timeline.len() > MAX_TIMELINE { state.timeline.remove(0); }
}

fn close_current_session(state: &mut TrackingState, ended_at: String, end_reason: &str) {
    if let Some(started_at) = state.current_session_started_at.take() {
        state.sessions.push(WorkSession {
            started_at, ended_at: Some(ended_at), active_seconds: state.current_session_active_seconds,
            idle_seconds: state.current_session_idle_seconds, switch_count: state.current_session_switches,
            app_count: state.current_session_apps.len() as u64, end_reason: Some(end_reason.into()),
        });
        if state.sessions.len() > MAX_SESSIONS { state.sessions.remove(0); }
    }
    state.current_session_active_seconds = 0;
    state.current_session_idle_seconds = 0;
    state.current_session_switches = 0;
    state.current_session_apps.clear();
}

fn clamp(value: f64, max: f64) -> f64 { if max <= 0.0 { 0.0 } else { (value / max).clamp(0.0, 1.0) } }

fn update_daily_feature(state: &mut TrackingState, now: chrono::DateTime<chrono::Local>) {
    let focus_minutes = state.focus_seconds as f64 / 60.0;
    let idle_minutes = state.idle_seconds as f64 / 60.0;
    let total_minutes = focus_minutes + idle_minutes;
    let session_seconds = state.sessions.iter().map(|s| s.active_seconds).sum::<u64>() + state.current_session_active_seconds;
    let session_count = state.sessions.len() as u64 + if state.current_session_started_at.is_some() { 1 } else { 0 };
    let max_hour = state.hourly_focus.iter().enumerate().max_by_key(|(_, seconds)| **seconds).map(|(hour, _)| hour as u8).unwrap_or(0);
    let work_start = state.hourly_focus.iter().position(|value| *value > 0).map(|hour| hour as u8);
    let work_end = state.hourly_focus.iter().rposition(|value| *value > 0).map(|hour| hour as u8);
    let work_span_hours = match (work_start, work_end) { (Some(start), Some(end)) if end >= start => (end - start + 1) as f64, _ => 0.0 };
    let total_app_millis = state.apps.iter().map(|app| app.active_millis.max(app.seconds.saturating_mul(1000))).sum::<u64>().max(1) as f64;
    let mut shares: Vec<f64> = state.apps.iter().map(|app| app.active_millis.max(app.seconds.saturating_mul(1000)) as f64 / total_app_millis).collect();
    shares.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    shares.truncate(12);
    state.daily_feature = DailyFeatureVector {
        schema_version: 2, date: now.date_naive().to_string(), focus_minutes, idle_minutes,
        active_ratio: if total_minutes > 0.0 { focus_minutes / total_minutes } else { 0.0 },
        app_count: state.apps.len() as u64, context_switches: state.context_switches,
        switches_per_active_hour: if focus_minutes > 0.0 { state.context_switches as f64 / (focus_minutes / 60.0) } else { 0.0 },
        average_session_minutes: if session_count > 0 { session_seconds as f64 / 60.0 / session_count as f64 } else { 0.0 },
        session_count,
        keyboard_events_per_active_minute: if focus_minutes > 0.0 { state.keyboard_actions as f64 / focus_minutes } else { 0.0 },
        mouse_distance_per_active_minute: if focus_minutes > 0.0 { state.mouse_distance_px / focus_minutes } else { 0.0 },
        mouse_clicks_per_active_minute: if focus_minutes > 0.0 { state.mouse_clicks as f64 / focus_minutes } else { 0.0 },
        click_interval_variance_ms: if state.click_interval_count > 1 { state.click_interval_m2 / (state.click_interval_count - 1) as f64 } else { 0.0 },
        network_bytes_per_active_minute: if focus_minutes > 0.0 { (state.network_rx_bytes + state.network_tx_bytes) as f64 / focus_minutes } else { 0.0 },
        peak_focus_hour: max_hour, work_start_hour: work_start, work_end_hour: work_end, work_span_hours,
        app_time_share: shares, local_only: true,
    };
}

fn build_embedding(feature: &DailyFeatureVector) -> Vec<f64> {
    let mut vector = vec![0.0; EMBEDDING_DIMENSIONS];
    let peak_radians = feature.peak_focus_hour as f64 / 24.0 * std::f64::consts::TAU;
    vector[0] = clamp((1.0 + feature.focus_minutes).ln(), (1.0_f64 + 480.0).ln());
    vector[1] = clamp(feature.idle_minutes, 480.0);
    vector[2] = feature.active_ratio.clamp(0.0, 1.0);
    vector[3] = clamp(feature.app_count as f64, 12.0);
    vector[4] = clamp(feature.switches_per_active_hour, 30.0);
    vector[5] = clamp(feature.average_session_minutes, 120.0);
    vector[6] = clamp(feature.session_count as f64, 24.0);
    vector[7] = clamp(feature.keyboard_events_per_active_minute, 120.0);
    vector[8] = clamp(feature.mouse_distance_per_active_minute, 12_000.0);
    vector[9] = clamp(feature.mouse_clicks_per_active_minute, 20.0);
    vector[10] = clamp(feature.click_interval_variance_ms.sqrt(), 4_000.0);
    vector[11] = clamp((1.0 + feature.network_bytes_per_active_minute).ln(), (1.0_f64 + 1_000_000_000.0).ln());
    vector[12] = (peak_radians.sin() + 1.0) / 2.0;
    vector[13] = (peak_radians.cos() + 1.0) / 2.0;
    vector[14] = feature.work_start_hour.map(|hour| hour as f64 / 23.0).unwrap_or(0.0);
    vector[15] = clamp(feature.work_span_hours, 16.0);
    for index in 0..8 { vector[16 + index] = feature.app_time_share.get(index).copied().unwrap_or(0.0).clamp(0.0, 1.0); }
    vector
}

fn confidence(feature: &DailyFeatureVector) -> f64 { clamp(feature.focus_minutes, 120.0) }
fn cosine_similarity(left: &[f64], right: &[f64]) -> f64 {
    if left.len() != right.len() || left.is_empty() { return 0.0; }
    let (mut dot, mut left_norm, mut right_norm) = (0.0, 0.0, 0.0);
    for (a, b) in left.iter().zip(right.iter()) { dot += a * b; left_norm += a * a; right_norm += b * b; }
    if left_norm == 0.0 || right_norm == 0.0 { 0.0 } else { (dot / (left_norm.sqrt() * right_norm.sqrt())).clamp(0.0, 1.0) }
}

fn make_embedding_record(feature: &DailyFeatureVector, now: chrono::DateTime<chrono::Local>) -> EmbeddingRecord {
    EmbeddingRecord { schema_version: 1, date: feature.date.clone(), created_at: now.to_rfc3339(), dimensions: EMBEDDING_DIMENSIONS, embedding: build_embedding(feature), data_confidence: confidence(feature), feature: feature.clone() }
}

fn upsert_current_embedding(state: &mut TrackingState, now: chrono::DateTime<chrono::Local>) {
    if !state.embedding_enabled || state.daily_feature.focus_minutes <= 0.0 { return; }
    let record = make_embedding_record(&state.daily_feature, now);
    if let Some(existing) = state.embedding_history.iter_mut().find(|item| item.date == record.date) { *existing = record; }
    else { state.embedding_history.push(record); state.embedding_history.sort_by(|a, b| a.date.cmp(&b.date)); }
    if state.embedding_history.len() > MAX_EMBEDDING_HISTORY { state.embedding_history.remove(0); }
}

fn reset_daily_activity(state: &mut TrackingState) {
    state.active_window.clear(); state.keyboard_actions = 0; state.mouse_actions = 0; state.app_activation_count = 0; state.focus_seconds = 0;
    state.hourly_focus = [0; 24]; state.apps.clear(); state.timeline.clear(); state.minute_snapshots.clear(); state.flow_minutes.clear(); state.active_app_count = 0;
    state.mouse_distance_px = 0.0; state.mouse_clicks = 0; state.avg_click_interval_ms = 0; state.click_interval_count = 0;
    state.click_interval_mean_ms = 0.0; state.click_interval_m2 = 0.0; state.network_rx_bytes = 0; state.network_tx_bytes = 0;
    state.notification_count = 0; state.idle_seconds = 0; state.context_switches = 0; state.sessions.clear();
    state.current_session_started_at = None; state.current_session_active_seconds = 0; state.current_session_idle_seconds = 0;
    state.current_session_switches = 0; state.current_session_apps.clear(); state.last_input_at = None; state.last_input_age_seconds = 0;
    state.resume_latency_seconds = 0; state.daily_feature = DailyFeatureVector { schema_version: 2, local_only: true, ..Default::default() };
}

fn rollover_if_needed(state: &mut TrackingState, now: chrono::DateTime<chrono::Local>) {
    let today = now.date_naive().to_string();
    if state.collection_day.is_empty() {
        state.collection_day = if state.daily_feature.date.is_empty() { today.clone() } else { state.daily_feature.date.clone() };
    }
    if state.collection_day != today {
        let previous_day = state.collection_day.clone();
        update_daily_feature(state, now);
        state.daily_feature.date = previous_day;
        upsert_current_embedding(state, now);
        reset_daily_activity(state);
        state.collection_day = today.clone();
        state.daily_feature.date = today;
    }
}

fn insight(level: &str, title: &str, detail: String) -> PatternInsight { PatternInsight { level: level.into(), title: title.into(), detail } }

fn analyze_embedding(state: &TrackingState) -> EmbeddingAnalysis {
    if !state.embedding_enabled {
        return EmbeddingAnalysis { enabled: false, local_only: true, dimensions: EMBEDDING_DIMENSIONS, history_days: state.embedding_history.len(), notice: "임베딩 분석이 꺼져 있습니다. 활성화하면 수치형 특징 24개만 이 기기에 저장해 유사 업무일과 기준선을 비교합니다.".into(), ..Default::default() };
    }
    let current = make_embedding_record(&state.daily_feature, chrono::Local::now());
    if current.feature.focus_minutes <= 0.0 {
        return EmbeddingAnalysis { enabled: true, local_only: true, dimensions: EMBEDDING_DIMENSIONS, history_days: state.embedding_history.len(), current: Some(current), notice: "분석에는 최소 한 번의 실제 활동이 필요합니다. 창을 활성화한 상태로 작업하면 로컬 특징 벡터가 생성됩니다.".into(), ..Default::default() };
    }
    let prior: Vec<&EmbeddingRecord> = state.embedding_history.iter().filter(|item| item.date != current.date && item.embedding.len() == EMBEDDING_DIMENSIONS).collect();
    let mut similar: Vec<SimilarDay> = prior.iter().map(|item| SimilarDay {
        date: item.date.clone(), similarity: cosine_similarity(&current.embedding, &item.embedding),
        focus_delta_minutes: current.feature.focus_minutes - item.feature.focus_minutes,
        switch_delta: current.feature.switches_per_active_hour - item.feature.switches_per_active_hour,
        active_ratio_delta: current.feature.active_ratio - item.feature.active_ratio,
    }).collect();
    similar.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap_or(std::cmp::Ordering::Equal));
    similar.truncate(3);
    let mut insights = Vec::new();
    let baseline_similarity = if prior.is_empty() { None } else {
        let mut mean = vec![0.0; EMBEDDING_DIMENSIONS];
        for item in &prior { for (index, value) in item.embedding.iter().enumerate() { mean[index] += value; } }
        for value in &mut mean { *value /= prior.len() as f64; }
        Some(cosine_similarity(&current.embedding, &mean))
    };
    if !prior.is_empty() {
        let average_focus = prior.iter().map(|item| item.feature.focus_minutes).sum::<f64>() / prior.len() as f64;
        let average_switches = prior.iter().map(|item| item.feature.switches_per_active_hour).sum::<f64>() / prior.len() as f64;
        let average_active_ratio = prior.iter().map(|item| item.feature.active_ratio).sum::<f64>() / prior.len() as f64;
        if current.feature.focus_minutes >= average_focus * 1.2 && current.feature.focus_minutes - average_focus >= 10.0 { insights.push(insight("positive", "평소보다 긴 집중 시간", format!("오늘 집중 시간은 기준선보다 {:.0}분 길게 기록되었습니다.", current.feature.focus_minutes - average_focus))); }
        if current.feature.focus_minutes <= average_focus * 0.75 && average_focus - current.feature.focus_minutes >= 10.0 { insights.push(insight("attention", "집중 시간 감소", format!("오늘 집중 시간은 기준선보다 {:.0}분 짧습니다. 유휴 구간과 세션 분할을 함께 확인해 보세요.", average_focus - current.feature.focus_minutes))); }
        if current.feature.switches_per_active_hour >= average_switches * 1.25 && current.feature.switches_per_active_hour - average_switches >= 2.0 { insights.push(insight("attention", "컨텍스트 전환 증가", format!("활성 1시간당 전환이 기준선보다 {:.1}회 많습니다.", current.feature.switches_per_active_hour - average_switches))); }
        if current.feature.active_ratio <= average_active_ratio - 0.12 { insights.push(insight("attention", "유휴 비율 증가", format!("활성 비율이 기준선보다 {:.0}%p 낮습니다.", (average_active_ratio - current.feature.active_ratio) * 100.0))); }
        if let Some(best) = similar.first() { if best.similarity >= 0.90 { insights.push(insight("positive", "반복되는 업무 패턴", format!("{}의 업무 리듬과 {:.0}% 유사합니다. 앱 시간 분포와 세션 구조가 비슷합니다.", best.date, best.similarity * 100.0))); } }
    }
    if current.feature.app_time_share.first().copied().unwrap_or(0.0) >= 0.70 { insights.push(insight("positive", "단일 도구 집중", format!("가장 큰 앱 시간 비율이 {:.0}%입니다. 앱 이름이 아닌 사용 시간 비율만 분석했습니다.", current.feature.app_time_share[0] * 100.0))); }
    if current.feature.session_count >= 6 && current.feature.average_session_minutes <= 12.0 { insights.push(insight("attention", "짧게 분할된 세션", format!("{}개 세션의 평균 길이가 {:.1}분입니다. 짧은 재개와 전환이 반복되고 있습니다.", current.feature.session_count, current.feature.average_session_minutes))); }
    if insights.is_empty() { insights.push(insight("neutral", "기준선 학습 중", "이전 업무일이 더 쌓이면 현재 패턴과 기준선의 차이를 설명 가능한 지표로 제시합니다.".into())); }
    EmbeddingAnalysis { enabled: true, local_only: true, dimensions: EMBEDDING_DIMENSIONS, history_days: state.embedding_history.len(), current: Some(current), baseline_similarity, similar_days: similar, insights, notice: if prior.is_empty() { "첫 번째 로컬 임베딩입니다. 하루 이상 기록하면 유사 업무일과 개인 기준선을 비교할 수 있습니다.".into() } else { "분석은 이 기기에서 계산되며, 창 제목·키 입력·URL·화면 내용은 임베딩에 포함되지 않습니다.".into() } }
}

#[tauri::command]
fn set_tracking(enabled: bool, state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<TrackingState, String> {
    let mut current = state.lock().map_err(|_| "state unavailable")?;
    if current.enabled && !enabled && current.current_session_started_at.is_some() {
        let now = chrono::Local::now();
        let app = current.active_window.clone();
        record_event(&mut current, now.to_rfc3339(), app.clone(), app, "session_end_paused");
        close_current_session(&mut current, now.to_rfc3339(), "tracking_paused");
    }
    current.enabled = enabled;
    save(&current, &storage_path(&path)?)?;
    Ok(current.clone())
}
#[tauri::command]
fn get_tracking_state(state: tauri::State<'_, SharedState>) -> Result<TrackingState, String> { state.lock().map(|value| value.clone()).map_err(|_| "state unavailable".into()) }
#[tauri::command]
fn get_focus_experience_view(date: String, bucket_minutes: u8, state: tauri::State<'_, SharedState>) -> Result<FocusExperienceView, String> {
    if bucket_minutes != FOCUS_BUCKET_MINUTES { return Err(format!("only {FOCUS_BUCKET_MINUTES}-minute focus buckets are supported")); }
    let current = state.lock().map_err(|_| "state unavailable")?;
    let selected_date = if date.is_empty() { current.collection_day.clone() } else { date };
    if selected_date.is_empty() { return Err("collection day unavailable".into()); }
    if selected_date != current.collection_day { return Err("P0 focus experience currently supports the locally collected current day only".into()); }
    Ok(build_focus_experience_view(&current, &selected_date, bucket_minutes))
}
#[tauri::command]
fn get_daily_feature_vector(state: tauri::State<'_, SharedState>) -> Result<DailyFeatureVector, String> { state.lock().map(|value| value.daily_feature.clone()).map_err(|_| "state unavailable".into()) }
#[tauri::command]
fn get_embedding_analysis(state: tauri::State<'_, SharedState>) -> Result<EmbeddingAnalysis, String> { state.lock().map(|value| analyze_embedding(&value)).map_err(|_| "state unavailable".into()) }
#[tauri::command]
fn set_embedding_enabled(enabled: bool, state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<EmbeddingAnalysis, String> {
    let mut current = state.lock().map_err(|_| "state unavailable")?; current.embedding_enabled = enabled;
    if enabled { let now = chrono::Local::now(); update_daily_feature(&mut current, now); upsert_current_embedding(&mut current, now); }
    save(&current, &storage_path(&path)?)?; Ok(analyze_embedding(&current))
}
#[tauri::command]
fn clear_embedding_data(state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<(), String> { let mut current = state.lock().map_err(|_| "state unavailable")?; current.embedding_history.clear(); current.embedding_enabled = false; save(&current, &storage_path(&path)?)?; Ok(()) }
#[tauri::command]
fn clear_all_data(state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<(), String> { let mut current = state.lock().map_err(|_| "state unavailable")?; let enabled = current.enabled; *current = TrackingState { enabled, ..Default::default() }; let file = storage_path(&path)?; if file.exists() { fs::remove_file(file).map_err(|e| e.to_string())?; } Ok(()) }
#[tauri::command]
fn request_notification_access(state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<AdvancedCollectionState, String> { let mut current = state.lock().map_err(|_| "state unavailable")?; current.advanced.notification_requested = true; current.advanced.notification_access = "requires_msix_user_notification_listener_consent".into(); current.advanced.helper_last_error = "알림 수집은 관리자 권한이 아니라 MSIX 패키지 identity와 Windows 알림 접근 동의가 필요합니다.".into(); save(&current, &storage_path(&path)?)?; Ok(current.advanced.clone()) }
#[tauri::command]
fn request_per_app_network_collection(state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<AdvancedCollectionState, String> { let mut current = state.lock().map_err(|_| "state unavailable")?; current.advanced.per_app_network_requested = true; current.advanced.per_app_network_status = "admin_required_etw_helper_not_installed".into(); current.advanced.helper_last_error = "앱별 네트워크 바이트는 시스템 ETW 세션을 사용하므로 관리자 권한으로 설치되는 선택적 수집기가 필요합니다.".into(); save(&current, &storage_path(&path)?)?; Ok(current.advanced.clone()) }

#[cfg(windows)]
#[derive(Clone)]
struct ForegroundWindow {
    identity: String,
    app: String,
    title: String,
    process_resolved: bool,
}

#[cfg(windows)]
fn executable_name(process_id: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?;
    let mut buffer = [0u16; 32_768];
    let mut size = buffer.len() as u32;
    let result = unsafe { QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut size) };
    let _ = unsafe { CloseHandle(handle) };
    result.ok()?;
    let path = String::from_utf16_lossy(&buffer[..size as usize]);
    path.rsplit(|character| character == '\\' || character == '/').next().filter(|value| !value.is_empty()).map(str::to_string)
}

#[cfg(windows)]
fn foreground_window(process_cache: &mut HashMap<u32, (String, bool)>) -> Option<ForegroundWindow> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd == HWND::default() { return None; }
    let mut process_id = 0u32;
    if unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) } == 0 || process_id == 0 { return None; }
    let mut buffer = [0u16; 512];
    let length = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    let title = if length > 0 { String::from_utf16_lossy(&buffer[..length as usize]) } else { "제목 없음".into() };
    let (app, process_resolved) = process_cache.entry(process_id).or_insert_with(|| {
        executable_name(process_id).map(|name| (name, true)).unwrap_or_else(|| (format!("보호된 프로세스 (PID {process_id})"), false))
    }).clone();
    Some(ForegroundWindow { identity: format!("{process_id}:{:p}", hwnd.0), app, title, process_resolved })
}

#[cfg(windows)]
unsafe extern "system" fn count_visible_window(hwnd: windows::Win32::Foundation::HWND, lparam: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::BOOL {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
    if IsWindowVisible(hwnd).as_bool() { let count = &mut *(lparam.0 as *mut u64); *count += 1; }
    BOOL(1)
}

#[cfg(windows)]
fn visible_app_count() -> u64 {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
    let mut count = 0u64;
    let _ = unsafe { EnumWindows(Some(count_visible_window), LPARAM((&mut count as *mut u64) as isize)) };
    count
}

#[cfg(windows)]
fn network_totals() -> (u64, u64) {
    use std::ffi::c_void;
    use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    let result = unsafe { GetIfTable2(&mut table) };
    if result.0 != 0 || table.is_null() { return (0, 0); }
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
fn network_totals() -> (u64, u64) { (0, 0) }

#[cfg(windows)]
fn start_tracker(state: SharedState, path: SharedPath) {
    use chrono::{Local, Timelike};
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    thread::spawn(move || {
        let mut previous_identity = String::new();
        let mut previous_cursor = POINT { x: 0, y: 0 };
        let mut previous_keys = [false; 255];
        let mut previous_left = false;
        let mut previous_right = false;
        let mut last_click: Option<Instant> = None;
        let mut last_second = Instant::now();
        let mut last_foreground_sample = Instant::now();
        let mut last_save = Instant::now();
        let mut last_minute = Local::now().minute();
        let mut previous_network = network_totals();
        let mut last_input = Instant::now();
        let mut process_cache: HashMap<u32, (String, bool)> = HashMap::new();
        loop {
            thread::sleep(Duration::from_millis(50));
            if !state.lock().map(|value| value.enabled).unwrap_or(false) {
                last_foreground_sample = Instant::now();
                last_second = Instant::now();
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
                if down && !previous_keys[key] { key_presses += 1; }
                previous_keys[key] = down;
            }
            let mut input_seen = key_presses > 0 || click_started;
            let input_now = Local::now();
            if let Ok(mut current) = state.lock() {
                rollover_if_needed(&mut current, input_now);
                current.keyboard_actions += key_presses;
                let mut flow_mouse_actions = 0u32;
                let mut flow_mouse_distance_px = 0.0f32;
                if cursor_ok {
                    let dx = (cursor.x - previous_cursor.x) as f64;
                    let dy = (cursor.y - previous_cursor.y) as f64;
                    let distance = (dx * dx + dy * dy).sqrt();
                    if distance > 0.0 {
                        current.mouse_actions += 1;
                        current.mouse_distance_px += distance;
                        flow_mouse_actions = flow_mouse_actions.saturating_add(1);
                        flow_mouse_distance_px = distance.min(f32::MAX as f64) as f32;
                        input_seen = true;
                    }
                    previous_cursor = cursor;
                }
                if click_started {
                    current.mouse_clicks += 1;
                    flow_mouse_actions = flow_mouse_actions.saturating_add(1);
                    if let Some(previous) = last_click {
                        let interval = previous.elapsed().as_millis() as u64;
                        current.click_interval_count += 1;
                        let delta = interval as f64 - current.click_interval_mean_ms;
                        current.click_interval_mean_ms += delta / current.click_interval_count as f64;
                        current.click_interval_m2 += delta * (interval as f64 - current.click_interval_mean_ms);
                        current.avg_click_interval_ms = current.click_interval_mean_ms.round() as u64;
                    }
                    last_click = Some(Instant::now());
                }
                record_flow_input(&mut current, input_now, key_presses, flow_mouse_actions, flow_mouse_distance_px);
                if input_seen {
                    last_input = Instant::now();
                    current.last_input_at = Some(input_now.to_rfc3339());
                    current.last_input_age_seconds = 0;
                } else { current.last_input_age_seconds = last_input.elapsed().as_secs(); }
            }

            if last_foreground_sample.elapsed() >= Duration::from_millis(FOREGROUND_SAMPLE_MS) {
                let elapsed_ms = last_foreground_sample.elapsed().as_millis().min(1_000) as u64;
                let now = Local::now();
                let idle = last_input.elapsed().as_secs() >= IDLE_THRESHOLD_SECS;
                let foreground = foreground_window(&mut process_cache);
                if let Ok(mut current) = state.lock() {
                    rollover_if_needed(&mut current, now);
                    if let Some(window) = foreground {
                        let switched = window.identity != previous_identity;
                        current.active_window = window.app.clone();
                        if switched {
                            if !window.process_resolved { current.unresolved_window_count = current.unresolved_window_count.saturating_add(1); }
                            current.app_activation_count = current.app_activation_count.saturating_add(1);
                            current.context_switches = current.context_switches.saturating_add(1);
                            current.current_session_switches = current.current_session_switches.saturating_add(1);
                            if !current.current_session_apps.iter().any(|app| app == &window.app) { current.current_session_apps.push(window.app.clone()); }
                            record_event(&mut current, now.to_rfc3339(), window.app.clone(), window.title.clone(), "app_switch");
                            let minute = flow_minute_for(&mut current, now);
                            minute.switch_count = minute.switch_count.saturating_add(1);
                            let item = ensure_app(&mut current, &window.app, &window.title);
                            item.activations = item.activations.saturating_add(1);
                            previous_identity = window.identity;
                        }
                        if !idle { record_app_focus(&mut current, &window.app, &window.title, elapsed_ms, false); }
                    } else {
                        current.active_window = "활성 창을 식별할 수 없음".into();
                        previous_identity.clear();
                    }
                }
                last_foreground_sample = Instant::now();
            }

            if last_second.elapsed() >= Duration::from_secs(1) {
                let now = Local::now();
                let idle = last_input.elapsed().as_secs() >= IDLE_THRESHOLD_SECS;
                if let Ok(mut current) = state.lock() {
                    rollover_if_needed(&mut current, now);
                    if idle {
                        current.idle_seconds += 1;
                        current.current_session_idle_seconds += 1;
                    } else {
                        current.focus_seconds += 1;
                        current.current_session_active_seconds += 1;
                        current.hourly_focus[now.hour() as usize] += 1;
                    }
                    let minute = flow_minute_for(&mut current, now);
                    minute.observed_seconds = minute.observed_seconds.saturating_add(1);
                    if idle { minute.idle_seconds = minute.idle_seconds.saturating_add(1); }
                    else { minute.focus_seconds = minute.focus_seconds.saturating_add(1); }
                    let active_app = current.active_window.clone();
                    if !idle && current.current_session_started_at.is_none() {
                        current.resume_latency_seconds = current.last_input_age_seconds;
                        current.current_session_started_at = Some(now.to_rfc3339());
                        record_event(&mut current, now.to_rfc3339(), active_app.clone(), active_app.clone(), "session_start");
                    }
                    if idle && current.current_session_started_at.is_some() {
                        record_event(&mut current, now.to_rfc3339(), active_app.clone(), active_app.clone(), "session_end_idle");
                        close_current_session(&mut current, now.to_rfc3339(), "idle");
                    }
                    update_daily_feature(&mut current, now);
                }
                last_second = Instant::now();
            }

            let now = Local::now();
            if now.minute() != last_minute {
                let network = network_totals();
                let received_delta = network.0.saturating_sub(previous_network.0);
                let sent_delta = network.1.saturating_sub(previous_network.1);
                previous_network = network;
                if let Ok(mut current) = state.lock() {
                    current.active_app_count = visible_app_count();
                    current.network_rx_bytes = current.network_rx_bytes.saturating_add(received_delta);
                    current.network_tx_bytes = current.network_tx_bytes.saturating_add(sent_delta);
                    let snapshot = MinuteSnapshot { at: now.to_rfc3339(), active_app_count: current.active_app_count, focus_seconds: current.focus_seconds, keyboard_actions: current.keyboard_actions, mouse_actions: current.mouse_actions, mouse_distance_px: current.mouse_distance_px, network_rx_bytes: current.network_rx_bytes, network_tx_bytes: current.network_tx_bytes, idle_seconds: current.idle_seconds, context_switches: current.context_switches };
                    current.minute_snapshots.push(snapshot);
                    if current.minute_snapshots.len() > MAX_MINUTE_SNAPSHOTS { current.minute_snapshots.remove(0); }
                    update_daily_feature(&mut current, now);
                    upsert_current_embedding(&mut current, now);
                }
                if process_cache.len() > 512 { process_cache.clear(); }
                last_minute = now.minute();
            }
            if last_save.elapsed() >= Duration::from_secs(1) {
                if let (Ok(current), Ok(file)) = (state.lock(), storage_path(&path)) { let _ = save(&current, &file); }
                last_save = Instant::now();
            }
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
        if let Ok(mut current) = setup_state.lock() { *current = load(&file); let now = chrono::Local::now(); rollover_if_needed(&mut current, now); if current.collection_day.is_empty() { current.collection_day = now.date_naive().to_string(); current.daily_feature.date = current.collection_day.clone(); } }
        if let Ok(mut current_path) = setup_path.lock() { *current_path = file; }
        start_tracker(setup_state.clone(), setup_path.clone()); Ok(())
    }).manage(state).manage(path).invoke_handler(tauri::generate_handler![set_tracking, get_tracking_state, get_focus_experience_view, get_daily_feature_vector, get_embedding_analysis, set_embedding_enabled, clear_embedding_data, clear_all_data, request_notification_access, request_per_app_network_collection]).run(tauri::generate_context!()).expect("error while running FlowLens");
}


#[cfg(test)]
mod focus_experience_tests {
    use super::*;

    fn minute(at: &str, focus_seconds: u16, idle_seconds: u16, switch_count: u16, inputs: u32) -> FlowMinute {
        FlowMinute {
            start_at: at.into(), observed_seconds: focus_seconds.saturating_add(idle_seconds), focus_seconds, idle_seconds,
            switch_count, keyboard_actions: inputs, mouse_actions: 0, mouse_distance_px: 0.0,
        }
    }

    #[test]
    fn builds_five_minute_focus_and_switching_buckets() {
        let mut state = TrackingState::default();
        state.collection_day = "2026-09-17".into();
        state.flow_minutes = vec![
            minute("2026-09-17T09:00:00+09:00", 60, 0, 0, 12),
            minute("2026-09-17T09:01:00+09:00", 60, 0, 1, 12),
            minute("2026-09-17T09:02:00+09:00", 60, 0, 0, 12),
            minute("2026-09-17T09:03:00+09:00", 60, 0, 1, 12),
            minute("2026-09-17T09:04:00+09:00", 60, 0, 0, 12),
            minute("2026-09-17T10:00:00+09:00", 60, 0, 4, 8),
            minute("2026-09-17T11:00:00+09:00", 10, 50, 0, 0),
        ];
        let view = build_focus_experience_view(&state, "2026-09-17", 5);
        let focused = &view.ribbon.buckets[108];
        let switching = &view.ribbon.buckets[120];
        assert_eq!(focused.state, "focused");
        assert_eq!(focused.focus_seconds, 300);
        assert_eq!(focused.switch_count, 2);
        assert_eq!(switching.state, "switching");
        assert_eq!(view.ribbon.buckets[132].state, "idle");
        assert_eq!(view.ribbon.buckets[0].state, "unobserved");
        assert_eq!(view.ribbon.summary.longest_focused_span_seconds, 300);
    }

    #[test]
    fn includes_completed_and_live_session_cards_in_reverse_time_order() {
        let mut state = TrackingState::default();
        state.collection_day = "2026-09-17".into();
        state.sessions.push(WorkSession {
            started_at: "2026-09-17T09:00:00+09:00".into(), ended_at: Some("2026-09-17T09:30:00+09:00".into()),
            active_seconds: 1_800, idle_seconds: 0, switch_count: 2, app_count: 2, end_reason: Some("idle".into()),
        });
        state.current_session_started_at = Some("2026-09-17T10:00:00+09:00".into());
        state.current_session_active_seconds = 900;
        state.current_session_switches = 1;
        state.current_session_apps = vec!["editor.exe".into()];
        let view = build_focus_experience_view(&state, "2026-09-17", 5);
        assert_eq!(view.sessions.sessions.len(), 2);
        assert!(view.sessions.sessions[0].is_live);
        assert_eq!(view.sessions.sessions[0].status, "steady");
        assert_eq!(view.sessions.summary.completed_count, 1);
    }
}
