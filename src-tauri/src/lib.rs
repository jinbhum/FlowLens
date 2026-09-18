use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};
use tauri::Manager;

const IDLE_THRESHOLD_SECS: u64 = 5 * 60;
const MAX_TIMELINE: usize = 10_000;
const MAX_MINUTE_SNAPSHOTS: usize = 43_200;
const MAX_FLOW_MINUTES: usize = 1_440;
const MAX_SESSIONS: usize = 500;
const MAX_DAILY_REPORTS: usize = 365;
const MAX_REENTRY_EPISODES: usize = 64;
const FOCUS_BUCKET_MINUTES: u8 = 5;
const REENTRY_MIN_BREAK_SECS: i64 = 3 * 60;
const REENTRY_MAX_BREAK_SECS: i64 = 60 * 60;
const REENTRY_OBSERVATION_SECS: i64 = 10 * 60;
const MAX_EMBEDDING_HISTORY: usize = 365;
const EMBEDDING_DIMENSIONS: usize = 25;
const FOREGROUND_SAMPLE_MS: u64 = 200;
const DISK_SNAPSHOT_INTERVAL_SECS: i64 = 30 * 60;
const MAX_DISK_SNAPSHOTS: usize = 51_840;

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
    mouse_wheel_notches: u64,
    network_rx_bytes: u64,
    network_tx_bytes: u64,
    idle_seconds: u64,
    context_switches: u64,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct DiskUsageSnapshot {
    at: String,
    drive: String,
    total_bytes: u64,
    free_bytes: u64,
    used_bytes: u64,
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
    mouse_wheel_notches_per_active_minute: f64,
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
    last_workday_evaluation: Vec<String>,
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

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct PendingReentry {
    break_started_at: String,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct ReentryEpisode {
    id: String,
    break_started_at: String,
    resumed_at: String,
    break_seconds: u64,
    stabilization_seconds: Option<u64>,
    focused_seconds_first_10m: u16,
    switch_count_first_10m: u16,
    finalized: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct ReportSettings {
    enabled: bool,
    work_mode_tag: Option<String>,
    reflection_enabled: bool,
    flow_satisfaction: Option<u8>,
    reminder_enabled: bool,
}

impl Default for ReportSettings {
    fn default() -> Self {
        Self { enabled: false, work_mode_tag: None, reflection_enabled: false, flow_satisfaction: None, reminder_enabled: false }
    }
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct DailyContext {
    work_mode_tag: Option<String>,
    reflection_enabled: bool,
    flow_satisfaction: Option<u8>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct ReportDataQuality {
    observed_minutes: u16,
    active_minutes: u16,
    observed_coverage: f32,
    session_count: u16,
    reentry_episode_count: u16,
    comparison_sample_count: u16,
    level: String,
    limitations: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct DailyFlowMetrics {
    focus_seconds: u64,
    idle_seconds: u64,
    active_ratio: f32,
    work_span_minutes: u16,
    session_count: u16,
    median_session_seconds: u32,
    longest_session_seconds: u32,
    long_form_focus_share: f32,
    switch_count: u32,
    switches_per_active_hour: f32,
    short_session_share: f32,
    input_density_cv: Option<f32>,
    extended_session_count: u16,
    short_break_after_extended_count: u16,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct DayPartMetrics {
    part: String,
    observed_seconds: u32,
    focus_seconds: u32,
    idle_seconds: u32,
    switch_count: u16,
    input_actions: u32,
    focused_bucket_share: f32,
    switches_per_active_hour: f32,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct ReentryMetrics {
    analyzable_episode_count: u16,
    median_break_seconds: Option<u32>,
    median_stabilization_seconds: Option<u32>,
    stabilization_missing_share: Option<f32>,
    post_resume_switches_per_episode: Option<f32>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct EvidenceValue {
    metric: String,
    value: f64,
    unit: String,
    baseline_median: Option<f64>,
    baseline_sample_count: u16,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct ReportInsight {
    code: String,
    level: String,
    title: String,
    detail: String,
    evidence: Vec<EvidenceValue>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct DailyReportContent {
    headline: String,
    highlights: Vec<ReportInsight>,
    observations: Vec<ReportInsight>,
    limitations: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct BaselineDescriptor {
    cohort: String,
    sample_count: u16,
    date_range_start: Option<String>,
    date_range_end: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct FlowStrainSignal {
    code: String,
    label: String,
    state: String,
    current_value: Option<f64>,
    baseline_median: Option<f64>,
    robust_delta: Option<f32>,
    evidence: Vec<EvidenceValue>,
    explanation: String,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct FlowStrainAssessment {
    eligible: bool,
    baseline: BaselineDescriptor,
    signals: Vec<FlowStrainSignal>,
    signal_count: u8,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct DailyReportRecord {
    schema_version: u32,
    date: String,
    generated_at: String,
    status: String,
    data_quality: ReportDataQuality,
    context: DailyContext,
    metrics: DailyFlowMetrics,
    day_parts: Vec<DayPartMetrics>,
    reentry: ReentryMetrics,
    strain: FlowStrainAssessment,
    report: DailyReportContent,
}

#[derive(Clone, Serialize)]
struct DailyReportListItem {
    date: String,
    status: String,
    headline: String,
    data_quality_level: String,
    signal_count: u8,
    work_mode_tag: Option<String>,
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
    mouse_wheel_notches: u64,
    disk_snapshots: Vec<DiskUsageSnapshot>,
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
    reentry_episodes: Vec<ReentryEpisode>,
    pending_reentry: Option<PendingReentry>,
    daily_reports: Vec<DailyReportRecord>,
    report_settings: ReportSettings,
}

impl Default for TrackingState {
    fn default() -> Self {
        Self {
            enabled: true, active_window: String::new(), keyboard_actions: 0, mouse_actions: 0,
            app_activation_count: 0, focus_seconds: 0, hourly_focus: [0; 24], apps: Vec::new(),
            timeline: Vec::new(), minute_snapshots: Vec::new(), flow_minutes: Vec::new(), active_app_count: 0,
            mouse_distance_px: 0.0, mouse_clicks: 0, mouse_wheel_notches: 0, disk_snapshots: Vec::new(), avg_click_interval_ms: 0,
            click_interval_count: 0, click_interval_mean_ms: 0.0, click_interval_m2: 0.0,
            network_rx_bytes: 0, network_tx_bytes: 0, notification_count: 0, idle_seconds: 0,
            context_switches: 0, sessions: Vec::new(), current_session_started_at: None,
            current_session_active_seconds: 0, current_session_idle_seconds: 0,
            current_session_switches: 0, current_session_apps: Vec::new(),
            last_input_at: None, last_input_age_seconds: 0, resume_latency_seconds: 0,
            collection_day: String::new(),
            daily_feature: DailyFeatureVector { schema_version: 3, local_only: true, ..Default::default() },
            embedding_enabled: false, embedding_history: Vec::new(),
            advanced: AdvancedCollectionState { notification_access: "not_requested".into(), per_app_network_status: "not_requested".into(), ..Default::default() },
            tracker_sample_interval_ms: FOREGROUND_SAMPLE_MS, unresolved_window_count: 0,
            reentry_episodes: Vec::new(), pending_reentry: None, daily_reports: Vec::new(), report_settings: ReportSettings::default(),
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

fn migrate_local_state(state: &mut TrackingState) {
    state.daily_feature.schema_version = state.daily_feature.schema_version.max(3);
    for record in &mut state.embedding_history {
        record.embedding.resize(EMBEDDING_DIMENSIONS, 0.0);
        record.embedding.truncate(EMBEDDING_DIMENSIONS);
        record.dimensions = EMBEDDING_DIMENSIONS;
        record.schema_version = record.schema_version.max(2);
        record.feature.schema_version = record.feature.schema_version.max(3);
    }
}

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


fn parse_local_timestamp(value: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(value).ok()
}

fn report_date_matches(value: &str, date: &str) -> bool { value.starts_with(date) }

fn minute_of_day(item: &FlowMinute) -> Option<usize> {
    use chrono::Timelike;
    let timestamp = parse_local_timestamp(&item.start_at)?;
    Some(timestamp.hour() as usize * 60 + timestamp.minute() as usize)
}

fn flow_minutes_for_date<'a>(state: &'a TrackingState, date: &str) -> Vec<&'a FlowMinute> {
    let mut minutes: Vec<&FlowMinute> = state.flow_minutes.iter().filter(|item| report_date_matches(&item.start_at, date)).collect();
    minutes.sort_by_key(|item| minute_of_day(item).unwrap_or(usize::MAX));
    minutes
}

fn daily_sessions_for_date(state: &TrackingState, date: &str) -> Vec<WorkSession> {
    let mut sessions: Vec<WorkSession> = state.sessions.iter()
        .filter(|session| report_date_matches(&session.started_at, date))
        .cloned().collect();
    if let Some(started_at) = state.current_session_started_at.as_ref().filter(|value| report_date_matches(value, date)) {
        sessions.push(WorkSession {
            started_at: started_at.clone(), ended_at: None,
            active_seconds: state.current_session_active_seconds,
            idle_seconds: state.current_session_idle_seconds,
            switch_count: state.current_session_switches,
            app_count: state.current_session_apps.len() as u64,
            end_reason: None,
        });
    }
    sessions.sort_by(|left, right| left.started_at.cmp(&right.started_at));
    sessions
}

fn median_f64(mut values: Vec<f64>) -> Option<f64> {
    values.retain(|value| value.is_finite());
    if values.is_empty() { return None; }
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let middle = values.len() / 2;
    if values.len() % 2 == 0 { Some((values[middle - 1] + values[middle]) / 2.0) } else { Some(values[middle]) }
}

fn median_u64(values: Vec<u64>) -> Option<u64> { median_f64(values.into_iter().map(|value| value as f64).collect()).map(|value| value.round() as u64) }

fn coefficient_of_variation(values: &[f64]) -> Option<f32> {
    if values.len() < 2 { return None; }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    if mean <= 0.0 { return None; }
    let variance = values.iter().map(|value| (value - mean).powi(2)).sum::<f64>() / values.len() as f64;
    Some((variance.sqrt() / mean) as f32)
}

fn day_part(hour: usize) -> &'static str {
    match hour {
        5..=10 => "morning",
        11..=13 => "midday",
        14..=17 => "afternoon",
        _ => "evening",
    }
}

fn day_part_index(name: &str) -> usize {
    match name { "morning" => 0, "midday" => 1, "afternoon" => 2, _ => 3 }
}

fn current_daily_context(state: &TrackingState) -> DailyContext {
    DailyContext {
        work_mode_tag: state.report_settings.work_mode_tag.clone(),
        reflection_enabled: state.report_settings.reflection_enabled,
        flow_satisfaction: if state.report_settings.reflection_enabled { state.report_settings.flow_satisfaction } else { None },
    }
}

fn bucket_accumulators_for_date(state: &TrackingState, date: &str) -> Vec<FlowBucketAccumulator> {
    let mut buckets = vec![FlowBucketAccumulator::default(); 1_440 / FOCUS_BUCKET_MINUTES as usize];
    for minute in flow_minutes_for_date(state, date) {
        if let Some(slot) = flow_minute_slot(minute, date, FOCUS_BUCKET_MINUTES) {
            if let Some(bucket) = buckets.get_mut(slot) {
                bucket.observed_seconds = bucket.observed_seconds.saturating_add(minute.observed_seconds as u32);
                bucket.focus_seconds = bucket.focus_seconds.saturating_add(minute.focus_seconds as u32);
                bucket.idle_seconds = bucket.idle_seconds.saturating_add(minute.idle_seconds as u32);
                bucket.switch_count = bucket.switch_count.saturating_add(minute.switch_count as u32);
                bucket.input_actions = bucket.input_actions.saturating_add(minute.keyboard_actions.saturating_add(minute.mouse_actions));
            }
        }
    }
    buckets
}

fn assess_report_data_quality(state: &TrackingState, date: &str) -> ReportDataQuality {
    let minutes = flow_minutes_for_date(state, date);
    let observed_seconds = minutes.iter().map(|item| item.observed_seconds as u64).sum::<u64>();
    let active_seconds = minutes.iter().map(|item| item.focus_seconds as u64).sum::<u64>();
    let observed_slots: Vec<usize> = minutes.iter().filter(|item| item.observed_seconds > 0).filter_map(|item| minute_of_day(item)).collect();
    let span_seconds = match (observed_slots.iter().min(), observed_slots.iter().max()) {
        (Some(start), Some(end)) => ((end - start + 1) * 60) as u64,
        _ => 0,
    };
    let coverage = if span_seconds > 0 { (observed_seconds as f64 / span_seconds as f64).clamp(0.0, 1.0) as f32 } else { 0.0 };
    let observed_buckets = bucket_accumulators_for_date(state, date).iter().filter(|bucket| bucket.observed_seconds >= 30).count();
    let completed_sessions = state.sessions.iter().filter(|session| report_date_matches(&session.started_at, date)).count();
    let finalized_reentries = state.reentry_episodes.iter().filter(|episode| report_date_matches(&episode.resumed_at, date) && episode.finalized).count();
    let level = if observed_seconds < 60 * 60 || observed_buckets < 12 { "insufficient" }
        else if observed_seconds < 120 * 60 || completed_sessions < 2 || coverage < 0.60 { "partial" }
        else { "sufficient" };
    let mut limitations = Vec::new();
    if observed_seconds < 60 * 60 { limitations.push("관측된 활동 시간이 60분 미만이어서 개인 비교를 만들지 않았습니다.".into()); }
    if observed_buckets < 12 { limitations.push("5분 흐름 버킷이 충분하지 않아 시간대 비교의 신뢰도가 낮습니다.".into()); }
    if coverage < 0.60 && observed_seconds >= 60 * 60 { limitations.push("첫 관측부터 마지막 관측까지의 시간 중 실제 관측 비율이 낮습니다.".into()); }
    if completed_sessions < 2 && observed_seconds >= 60 * 60 { limitations.push("완료된 세션이 2개 미만이어서 세션 구조 비교를 제한합니다.".into()); }
    ReportDataQuality {
        observed_minutes: ((observed_seconds + 59) / 60).min(u16::MAX as u64) as u16,
        active_minutes: ((active_seconds + 59) / 60).min(u16::MAX as u64) as u16,
        observed_coverage: coverage,
        session_count: completed_sessions.min(u16::MAX as usize) as u16,
        reentry_episode_count: finalized_reentries.min(u16::MAX as usize) as u16,
        comparison_sample_count: 0,
        level: level.into(),
        limitations,
    }
}

fn aggregate_daily_flow_metrics(state: &TrackingState, date: &str) -> DailyFlowMetrics {
    let minutes = flow_minutes_for_date(state, date);
    let focus_seconds = minutes.iter().map(|item| item.focus_seconds as u64).sum::<u64>();
    let idle_seconds = minutes.iter().map(|item| item.idle_seconds as u64).sum::<u64>();
    let switch_count = minutes.iter().map(|item| item.switch_count as u64).sum::<u64>();
    let minute_slots: Vec<usize> = minutes.iter().filter(|item| item.observed_seconds > 0).filter_map(|item| minute_of_day(item)).collect();
    let work_span_minutes = match (minute_slots.iter().min(), minute_slots.iter().max()) {
        (Some(start), Some(end)) => (end - start + 1).min(u16::MAX as usize) as u16,
        _ => 0,
    };
    let sessions = daily_sessions_for_date(state, date);
    let session_lengths: Vec<u64> = sessions.iter().map(|session| session.active_seconds).collect();
    let session_seconds = session_lengths.iter().sum::<u64>();
    let long_form_seconds = sessions.iter().filter(|session| session.active_seconds >= 25 * 60).map(|session| session.active_seconds).sum::<u64>();
    let short_sessions = sessions.iter().filter(|session| session.active_seconds < 10 * 60).count();
    let extended_session_count = sessions.iter().filter(|session| session.active_seconds >= 90 * 60).count() as u16;
    let mut short_break_after_extended_count = 0u16;
    for pair in sessions.windows(2) {
        if pair[0].active_seconds < 90 * 60 { continue; }
        if let (Some(ended), Some(next)) = (parse_local_timestamp(pair[0].ended_at.as_deref().unwrap_or("")), parse_local_timestamp(&pair[1].started_at)) {
            let gap = next.timestamp().saturating_sub(ended.timestamp());
            if (0..15 * 60).contains(&gap) { short_break_after_extended_count = short_break_after_extended_count.saturating_add(1); }
        }
    }
    let input_densities: Vec<f64> = bucket_accumulators_for_date(state, date).iter()
        .filter(|bucket| bucket.observed_seconds >= 30)
        .map(|bucket| bucket.input_actions as f64 / (bucket.observed_seconds as f64 / 60.0).max(1.0))
        .collect();
    DailyFlowMetrics {
        focus_seconds,
        idle_seconds,
        active_ratio: (if focus_seconds + idle_seconds > 0 { focus_seconds as f64 / (focus_seconds + idle_seconds) as f64 } else { 0.0 }) as f32,
        work_span_minutes,
        session_count: sessions.len().min(u16::MAX as usize) as u16,
        median_session_seconds: median_u64(session_lengths.clone()).unwrap_or(0).min(u32::MAX as u64) as u32,
        longest_session_seconds: session_lengths.iter().copied().max().unwrap_or(0).min(u32::MAX as u64) as u32,
        long_form_focus_share: (if session_seconds > 0 { long_form_seconds as f64 / session_seconds as f64 } else { 0.0 }) as f32,
        switch_count: switch_count.min(u32::MAX as u64) as u32,
        switches_per_active_hour: (if focus_seconds > 0 { switch_count as f64 / (focus_seconds as f64 / 3600.0) } else { 0.0 }) as f32,
        short_session_share: (if sessions.is_empty() { 0.0 } else { short_sessions as f64 / sessions.len() as f64 }) as f32,
        input_density_cv: coefficient_of_variation(&input_densities),
        extended_session_count,
        short_break_after_extended_count,
    }
}

fn aggregate_day_parts(state: &TrackingState, date: &str) -> Vec<DayPartMetrics> {
    let mut parts = vec![DayPartMetrics { part: "morning".into(), ..Default::default() }, DayPartMetrics { part: "midday".into(), ..Default::default() }, DayPartMetrics { part: "afternoon".into(), ..Default::default() }, DayPartMetrics { part: "evening".into(), ..Default::default() }];
    for minute in flow_minutes_for_date(state, date) {
        if let Some(slot) = minute_of_day(minute) {
            let part = &mut parts[day_part_index(day_part(slot / 60))];
            part.observed_seconds = part.observed_seconds.saturating_add(minute.observed_seconds as u32);
            part.focus_seconds = part.focus_seconds.saturating_add(minute.focus_seconds as u32);
            part.idle_seconds = part.idle_seconds.saturating_add(minute.idle_seconds as u32);
            part.switch_count = part.switch_count.saturating_add(minute.switch_count);
            part.input_actions = part.input_actions.saturating_add(minute.keyboard_actions.saturating_add(minute.mouse_actions));
        }
    }
    let buckets = bucket_accumulators_for_date(state, date);
    let mut observed_buckets = [0u32; 4];
    let mut focused_buckets = [0u32; 4];
    for (index, bucket) in buckets.iter().enumerate() {
        let part_index = day_part_index(day_part((index * FOCUS_BUCKET_MINUTES as usize) / 60));
        if bucket.observed_seconds >= 30 {
            observed_buckets[part_index] = observed_buckets[part_index].saturating_add(1);
            if flow_state_and_intensity(bucket).0 == "focused" { focused_buckets[part_index] = focused_buckets[part_index].saturating_add(1); }
        }
    }
    for (index, part) in parts.iter_mut().enumerate() {
        part.focused_bucket_share = if observed_buckets[index] > 0 { focused_buckets[index] as f32 / observed_buckets[index] as f32 } else { 0.0 };
        part.switches_per_active_hour = (if part.focus_seconds > 0 { part.switch_count as f64 / (part.focus_seconds as f64 / 3600.0) } else { 0.0 }) as f32;
    }
    parts
}

fn reentry_outcome(flow_minutes: &[FlowMinute], episode: &ReentryEpisode, now: chrono::DateTime<chrono::Local>) -> Option<(Option<u64>, u16, u16, bool)> {
    use chrono::Timelike;
    let resumed = parse_local_timestamp(&episode.resumed_at)?;
    let date = resumed.date_naive().to_string();
    let start_slot = resumed.hour() as usize * 60 + resumed.minute() as usize;
    let mut by_slot: HashMap<usize, &FlowMinute> = HashMap::new();
    for minute in flow_minutes.iter().filter(|item| report_date_matches(&item.start_at, &date)) {
        if let Some(slot) = minute_of_day(minute) { by_slot.insert(slot, minute); }
    }
    let mut focused_seconds = 0u16;
    let mut switches = 0u16;
    let mut focused_streak = 0usize;
    let mut stabilization = None;
    for offset in 0..10usize {
        let item = by_slot.get(&(start_slot + offset));
        let is_focused = item.map(|minute| minute.observed_seconds >= 30 && minute.focus_seconds as f32 / minute.observed_seconds.max(1) as f32 >= 0.75 && minute.switch_count <= 1).unwrap_or(false);
        if let Some(minute) = item {
            focused_seconds = focused_seconds.saturating_add(minute.focus_seconds);
            switches = switches.saturating_add(minute.switch_count);
        }
        if is_focused {
            focused_streak += 1;
            if focused_streak >= 5 && stabilization.is_none() { stabilization = Some(((offset + 1) * 60) as u64); }
        } else { focused_streak = 0; }
    }
    let finalized = now.timestamp().saturating_sub(resumed.timestamp()) >= REENTRY_OBSERVATION_SECS;
    Some((stabilization, focused_seconds, switches, finalized))
}

fn refresh_reentry_episodes(state: &mut TrackingState, now: chrono::DateTime<chrono::Local>) {
    for index in 0..state.reentry_episodes.len() {
        if state.reentry_episodes[index].finalized { continue; }
        let episode = state.reentry_episodes[index].clone();
        if let Some((stabilization, focused_seconds, switches, finalized)) = reentry_outcome(&state.flow_minutes, &episode, now) {
            let current = &mut state.reentry_episodes[index];
            current.stabilization_seconds = stabilization;
            current.focused_seconds_first_10m = focused_seconds;
            current.switch_count_first_10m = switches;
            current.finalized = finalized;
        }
    }
}

fn begin_pending_reentry(state: &mut TrackingState, now: chrono::DateTime<chrono::Local>) {
    if state.report_settings.enabled { state.pending_reentry = Some(PendingReentry { break_started_at: now.to_rfc3339() }); }
}

fn begin_reentry_episode(state: &mut TrackingState, now: chrono::DateTime<chrono::Local>) {
    if !state.report_settings.enabled { return; }
    let pending = match state.pending_reentry.take() { Some(value) => value, None => return };
    let break_start = match parse_local_timestamp(&pending.break_started_at) { Some(value) => value, None => return };
    let break_seconds = now.timestamp().saturating_sub(break_start.timestamp());
    if !(REENTRY_MIN_BREAK_SECS..=REENTRY_MAX_BREAK_SECS).contains(&break_seconds) { return; }
    state.reentry_episodes.push(ReentryEpisode {
        id: format!("{}-{}", pending.break_started_at, now.timestamp()),
        break_started_at: pending.break_started_at,
        resumed_at: now.to_rfc3339(),
        break_seconds: break_seconds as u64,
        stabilization_seconds: None,
        focused_seconds_first_10m: 0,
        switch_count_first_10m: 0,
        finalized: false,
    });
    if state.reentry_episodes.len() > MAX_REENTRY_EPISODES { state.reentry_episodes.remove(0); }
}

fn aggregate_reentry_metrics(state: &TrackingState, date: &str) -> ReentryMetrics {
    let episodes: Vec<&ReentryEpisode> = state.reentry_episodes.iter().filter(|episode| report_date_matches(&episode.resumed_at, date) && episode.finalized).collect();
    if episodes.is_empty() { return ReentryMetrics::default(); }
    let breaks = episodes.iter().map(|episode| episode.break_seconds as f64).collect();
    let stabilizations: Vec<f64> = episodes.iter().filter_map(|episode| episode.stabilization_seconds.map(|value| value as f64)).collect();
    let missing = episodes.iter().filter(|episode| episode.stabilization_seconds.is_none()).count();
    let switches = episodes.iter().map(|episode| episode.switch_count_first_10m as u64).sum::<u64>();
    ReentryMetrics {
        analyzable_episode_count: episodes.len().min(u16::MAX as usize) as u16,
        median_break_seconds: median_f64(breaks).map(|value| value.round().min(u32::MAX as f64) as u32),
        median_stabilization_seconds: median_f64(stabilizations).map(|value| value.round().min(u32::MAX as f64) as u32),
        stabilization_missing_share: Some(missing as f32 / episodes.len() as f32),
        post_resume_switches_per_episode: Some(switches as f32 / episodes.len() as f32),
    }
}

struct BaselineSelection<'a> {
    descriptor: BaselineDescriptor,
    records: Vec<&'a DailyReportRecord>,
}

fn parsed_date(value: &str) -> Option<chrono::NaiveDate> { chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok() }

fn make_baseline_selection<'a>(cohort: &str, records: Vec<&'a DailyReportRecord>) -> BaselineSelection<'a> {
    let mut dates: Vec<String> = records.iter().map(|record| record.date.clone()).collect();
    dates.sort();
    BaselineSelection {
        descriptor: BaselineDescriptor {
            cohort: cohort.into(), sample_count: records.len().min(u16::MAX as usize) as u16,
            date_range_start: dates.first().cloned(), date_range_end: dates.last().cloned(),
        },
        records,
    }
}

fn select_baseline<'a>(reports: &'a [DailyReportRecord], settings: &ReportSettings, date: &str) -> BaselineSelection<'a> {
    use chrono::Datelike;
    let today = match parsed_date(date) {
        Some(value) => value,
        None => return make_baseline_selection("unavailable", Vec::new()),
    };
    let weekday = today.weekday();
    let candidates: Vec<&DailyReportRecord> = reports.iter().filter(|record| {
        if record.status != "finalized" || record.data_quality.level != "sufficient" || record.date == date { return false; }
        parsed_date(&record.date).map(|day| {
            let age = today.signed_duration_since(day).num_days();
            (1..=56).contains(&age)
        }).unwrap_or(false)
    }).collect();
    let tagged_weekday: Vec<&DailyReportRecord> = candidates.iter().copied().filter(|record| record.context.work_mode_tag == settings.work_mode_tag && settings.work_mode_tag.is_some() && parsed_date(&record.date).map(|day| day.weekday() == weekday).unwrap_or(false)).collect();
    if tagged_weekday.len() >= 8 { return make_baseline_selection("tag_weekday", tagged_weekday); }
    let tagged: Vec<&DailyReportRecord> = candidates.iter().copied().filter(|record| record.context.work_mode_tag == settings.work_mode_tag && settings.work_mode_tag.is_some()).collect();
    if tagged.len() >= 10 { return make_baseline_selection("tag", tagged); }
    let same_weekday: Vec<&DailyReportRecord> = candidates.iter().copied().filter(|record| parsed_date(&record.date).map(|day| day.weekday() == weekday).unwrap_or(false)).collect();
    if same_weekday.len() >= 10 { return make_baseline_selection("weekday", same_weekday); }
    if candidates.len() >= 14 { return make_baseline_selection("all_days", candidates); }
    make_baseline_selection("unavailable", Vec::new())
}

fn robust_comparison(current: f64, values: Vec<f64>, floor: f64) -> (Option<f64>, Option<f32>, u16) {
    let count = values.len().min(u16::MAX as usize) as u16;
    let median = match median_f64(values.clone()) { Some(value) => value, None => return (None, None, 0) };
    let deviations = values.into_iter().map(|value| (value - median).abs()).collect();
    let mad = median_f64(deviations).unwrap_or(0.0);
    let scale = (1.4826 * mad).max(floor);
    (Some(median), Some(((current - median) / scale) as f32), count)
}

fn metric_evidence(metric: &str, value: f64, unit: &str, baseline: Option<f64>, sample_count: u16) -> EvidenceValue {
    EvidenceValue { metric: metric.into(), value, unit: unit.into(), baseline_median: baseline, baseline_sample_count: sample_count }
}

fn unavailable_signal(code: &str, label: &str, explanation: &str) -> FlowStrainSignal {
    FlowStrainSignal { code: code.into(), label: label.into(), state: "not_evaluated".into(), current_value: None, baseline_median: None, robust_delta: None, evidence: Vec::new(), explanation: explanation.into() }
}

fn part<'a>(parts: &'a [DayPartMetrics], name: &str) -> Option<&'a DayPartMetrics> { parts.iter().find(|item| item.part == name) }

fn day_part_baseline_values(records: &[&DailyReportRecord], name: &str) -> Vec<f64> {
    records.iter().filter_map(|record| part(&record.day_parts, name)).filter(|item| item.observed_seconds > 0).map(|item| item.switches_per_active_hour as f64).collect()
}

fn assess_flow_strain(metrics: &DailyFlowMetrics, day_parts: &[DayPartMetrics], reentry: &ReentryMetrics, baseline: &BaselineSelection<'_>, quality: &ReportDataQuality) -> FlowStrainAssessment {
    let eligible = quality.level == "sufficient" && !baseline.records.is_empty();
    if !eligible {
        return FlowStrainAssessment {
            eligible: false, baseline: baseline.descriptor.clone(), signal_count: 0,
            signals: vec![
                unavailable_signal("late_fragmentation", "후반 전환 변화", "관측 품질 또는 개인 기준선 표본이 부족해 비교하지 않았습니다."),
                unavailable_signal("reentry_friction", "재진입 흐름", "관측 품질 또는 개인 기준선 표본이 부족해 비교하지 않았습니다."),
                unavailable_signal("extended_unbroken_flow", "긴 연속 활동", "관측 품질 또는 개인 기준선 표본이 부족해 비교하지 않았습니다."),
                unavailable_signal("rhythm_volatility", "상호작용 리듬 변화", "관측 품질 또는 개인 기준선 표본이 부족해 비교하지 않았습니다."),
            ],
        };
    }

    let afternoon = part(day_parts, "afternoon");
    let morning = part(day_parts, "morning");
    let late_signal = if let Some(afternoon) = afternoon.filter(|item| item.observed_seconds >= 45 * 60) {
        let (median, delta, sample) = robust_comparison(afternoon.switches_per_active_hour as f64, day_part_baseline_values(&baseline.records, "afternoon"), 1.0);
        let morning_increase = morning.map(|item| afternoon.switches_per_active_hour > item.switches_per_active_hour * 1.30).unwrap_or(false);
        let state = match delta {
            Some(value) if morning_increase && value >= 1.75 => "high",
            Some(value) if morning_increase && value >= 1.0 => "elevated",
            _ => "within_personal_range",
        };
        FlowStrainSignal {
            code: "late_fragmentation".into(), label: "후반 전환 변화".into(), state: state.into(),
            current_value: Some(afternoon.switches_per_active_hour as f64), baseline_median: median, robust_delta: delta,
            evidence: vec![metric_evidence("afternoon_switches_per_active_hour", afternoon.switches_per_active_hour as f64, "switches/hour", median, sample), metric_evidence("afternoon_observed_minutes", afternoon.observed_seconds as f64 / 60.0, "minutes", None, 0)],
            explanation: if state == "within_personal_range" { "후반 앱 전환 밀도는 현재 개인 기준선 범위에서 기록되었습니다.".into() } else { "오후 전환 밀도가 개인 기준보다 높고 오전보다도 증가했습니다. 이는 작업 흐름의 변화 신호이며 능력이나 건강 상태의 판단이 아닙니다.".into() },
        }
    } else { unavailable_signal("late_fragmentation", "후반 전환 변화", "오후 관측 시간이 45분 미만이어서 후반 전환을 비교하지 않았습니다.") };

    let reentry_signal = if reentry.analyzable_episode_count >= 2 {
        let current = reentry.median_stabilization_seconds.map(|value| value as f64);
        let values: Vec<f64> = baseline.records.iter().filter_map(|record| record.reentry.median_stabilization_seconds.map(|value| value as f64)).collect();
        let (median, delta, sample) = current.map(|value| robust_comparison(value, values, 60.0)).unwrap_or((None, None, 0));
        let missing = reentry.stabilization_missing_share.unwrap_or(0.0) >= 0.50;
        let current_switches = reentry.post_resume_switches_per_episode.unwrap_or(0.0) as f64;
        let (switch_median, switch_delta, _) = robust_comparison(current_switches, baseline.records.iter().filter_map(|record| record.reentry.post_resume_switches_per_episode.map(|value| value as f64)).collect(), 1.0);
        let state = match delta {
            Some(value) if value >= 1.75 && switch_delta.unwrap_or(0.0) > 0.0 => "high",
            Some(value) if value >= 1.0 || missing => "elevated",
            _ if missing => "elevated",
            _ => "within_personal_range",
        };
        FlowStrainSignal {
            code: "reentry_friction".into(), label: "재진입 흐름".into(), state: state.into(), current_value: current, baseline_median: median, robust_delta: delta,
            evidence: vec![metric_evidence("median_stabilization_seconds", current.unwrap_or(0.0), "seconds", median, sample), metric_evidence("post_resume_switches_per_episode", current_switches, "switches/episode", switch_median, baseline.descriptor.sample_count)],
            explanation: if state == "within_personal_range" { "분석 가능한 중단 뒤 재진입 흐름은 현재 개인 기준선 범위에서 기록되었습니다.".into() } else { "중단 뒤 연속 흐름이 형성되기까지의 시간 또는 재개 직후 전환이 평소보다 길게 기록되었습니다. 이는 작업 흐름의 관찰값입니다.".into() },
        }
    } else { unavailable_signal("reentry_friction", "재진입 흐름", "3~60분 중단 뒤 재개한 사례가 2개 미만이어서 비교하지 않았습니다.") };

    let extended_values: Vec<f64> = baseline.records.iter().map(|record| record.metrics.long_form_focus_share as f64).collect();
    let (extended_median, extended_delta, extended_sample) = robust_comparison(metrics.long_form_focus_share as f64, extended_values, 0.08);
    let extended_condition = metrics.extended_session_count >= 1 && metrics.short_break_after_extended_count >= 1;
    let extended_state = match extended_delta {
        Some(value) if extended_condition && (metrics.extended_session_count >= 2 || value >= 1.75) => "high",
        Some(_) if extended_condition => "elevated",
        _ => "within_personal_range",
    };
    let extended_signal = FlowStrainSignal {
        code: "extended_unbroken_flow".into(), label: "긴 연속 활동".into(), state: extended_state.into(),
        current_value: Some(metrics.long_form_focus_share as f64), baseline_median: extended_median, robust_delta: extended_delta,
        evidence: vec![metric_evidence("long_form_focus_share", metrics.long_form_focus_share as f64, "ratio", extended_median, extended_sample), metric_evidence("extended_session_count", metrics.extended_session_count as f64, "sessions", None, 0)],
        explanation: if extended_state == "within_personal_range" { "긴 연속 활동의 비중은 현재 개인 기준선 범위에서 기록되었습니다.".into() } else { "90분 이상 연속 활동 뒤 짧은 중단이 반복되었습니다. 이 값은 휴식 필요성이나 건강 상태를 판단하지 않고 세션 구조만 설명합니다.".into() },
    };

    let rhythm_signal = if let Some(current_cv) = metrics.input_density_cv {
        let (cv_median, cv_delta, cv_sample) = robust_comparison(current_cv as f64, baseline.records.iter().filter_map(|record| record.metrics.input_density_cv.map(|value| value as f64)).collect(), 0.10);
        let (switch_median, switch_delta, _) = robust_comparison(metrics.switches_per_active_hour as f64, baseline.records.iter().map(|record| record.metrics.switches_per_active_hour as f64).collect(), 1.0);
        let state = match (cv_delta, switch_delta) {
            (Some(cv), Some(sw)) if cv >= 1.75 && sw >= 1.75 => "high",
            (Some(cv), Some(sw)) if cv >= 1.0 && sw >= 1.0 => "elevated",
            _ => "within_personal_range",
        };
        FlowStrainSignal {
            code: "rhythm_volatility".into(), label: "상호작용 리듬 변화".into(), state: state.into(), current_value: Some(current_cv as f64), baseline_median: cv_median, robust_delta: cv_delta,
            evidence: vec![metric_evidence("input_density_cv", current_cv as f64, "coefficient", cv_median, cv_sample), metric_evidence("switches_per_active_hour", metrics.switches_per_active_hour as f64, "switches/hour", switch_median, baseline.descriptor.sample_count)],
            explanation: if state == "within_personal_range" { "입력 리듬과 전환 밀도는 현재 개인 기준선 범위에서 기록되었습니다.".into() } else { "입력 리듬의 변동과 앱 전환이 함께 증가했습니다. 입력량 자체는 작업 성과로 해석하지 않습니다.".into() },
        }
    } else { unavailable_signal("rhythm_volatility", "상호작용 리듬 변화", "관측된 5분 입력 버킷이 충분하지 않아 리듬 변동을 비교하지 않았습니다.") };

    let signals = vec![late_signal, reentry_signal, extended_signal, rhythm_signal];
    let signal_count = signals.iter().filter(|signal| signal.state == "elevated" || signal.state == "high").count().min(u8::MAX as usize) as u8;
    FlowStrainAssessment { eligible: true, baseline: baseline.descriptor.clone(), signals, signal_count }
}

fn render_daily_report_content(quality: &ReportDataQuality, metrics: &DailyFlowMetrics, strain: &FlowStrainAssessment) -> DailyReportContent {
    let headline = if quality.level == "insufficient" { "오늘의 기록을 더 모으면 흐름 요약을 만들 수 있습니다.".into() }
        else if !strain.eligible { "오늘의 작업 흐름을 기록했습니다. 개인 기준선은 더 쌓인 뒤 비교합니다.".into() }
        else if strain.signal_count == 0 { "오늘의 흐름은 현재 개인 기준선 범위에서 기록되었습니다.".into() }
        else if strain.signal_count == 1 { "오늘의 흐름에서 확인할 변화 신호가 1개 있습니다.".into() }
        else { "오늘의 후반 작업 흐름에서 여러 변화 신호가 함께 기록되었습니다.".into() };
    let highlights: Vec<ReportInsight> = strain.signals.iter().filter(|signal| signal.state == "elevated" || signal.state == "high").map(|signal| ReportInsight {
        code: signal.code.clone(), level: "notice".into(), title: signal.label.clone(), detail: signal.explanation.clone(), evidence: signal.evidence.clone(),
    }).collect();
    let observations = vec![ReportInsight {
        code: "daily_flow_summary".into(), level: "neutral".into(), title: "오늘의 흐름 요약".into(),
        detail: format!("집중 {:.0}분, 세션 {}개, 활성 1시간당 전환 {:.1}회가 로컬 집계되었습니다.", metrics.focus_seconds as f64 / 60.0, metrics.session_count, metrics.switches_per_active_hour),
        evidence: vec![metric_evidence("focus_minutes", metrics.focus_seconds as f64 / 60.0, "minutes", None, 0), metric_evidence("session_count", metrics.session_count as f64, "sessions", None, 0)],
    }];
    DailyReportContent { headline, highlights, observations, limitations: quality.limitations.clone() }
}

fn build_daily_report(state: &TrackingState, date: &str, status: &str, now: chrono::DateTime<chrono::Local>) -> DailyReportRecord {
    let mut quality = assess_report_data_quality(state, date);
    let metrics = aggregate_daily_flow_metrics(state, date);
    let day_parts = aggregate_day_parts(state, date);
    let reentry = aggregate_reentry_metrics(state, date);
    let baseline = select_baseline(&state.daily_reports, &state.report_settings, date);
    quality.comparison_sample_count = baseline.descriptor.sample_count;
    let strain = assess_flow_strain(&metrics, &day_parts, &reentry, &baseline, &quality);
    let report = render_daily_report_content(&quality, &metrics, &strain);
    DailyReportRecord {
        schema_version: 1, date: date.into(), generated_at: now.to_rfc3339(), status: status.into(),
        data_quality: quality, context: current_daily_context(state), metrics, day_parts, reentry, strain, report,
    }
}

fn upsert_daily_report(state: &mut TrackingState, report: DailyReportRecord) {
    if let Some(index) = state.daily_reports.iter().position(|item| item.date == report.date) { state.daily_reports[index] = report; }
    else { state.daily_reports.push(report); }
    state.daily_reports.sort_by(|left, right| left.date.cmp(&right.date));
    while state.daily_reports.len() > MAX_DAILY_REPORTS {
        if let Some(index) = state.daily_reports.iter().position(|item| item.status == "finalized") { state.daily_reports.remove(index); }
        else { state.daily_reports.remove(0); }
    }
}

fn allowed_work_mode_tag(tag: &str) -> bool { matches!(tag, "구현" | "디버깅" | "문서화" | "검토" | "회의" | "학습" | "운영 대응") }

fn finalize_local_state_for_exit(state: &mut TrackingState, now: chrono::DateTime<chrono::Local>) {
    rollover_if_needed(state, now);
    if state.current_session_started_at.is_some() {
        let active_app = state.active_window.clone();
        record_event(state, now.to_rfc3339(), active_app.clone(), active_app, "session_end_app_exit");
        close_current_session(state, now.to_rfc3339(), "app_exit");
        state.pending_reentry = None;
    }
    refresh_reentry_episodes(state, now);
    update_daily_feature(state, now);
    if state.report_settings.enabled && !state.collection_day.is_empty() {
        let date = state.collection_day.clone();
        let report = build_daily_report(state, &date, "draft", now);
        upsert_daily_report(state, report);
    }
    upsert_current_embedding(state, now);
}

fn persist_before_exit(state: &SharedState, path: &SharedPath) -> Result<(), String> {
    let mut current = state.lock().map_err(|_| "state unavailable")?;
    finalize_local_state_for_exit(&mut current, chrono::Local::now());
    save(&current, &storage_path(path)?)
}

fn show_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn build_system_tray<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    use tauri::{
        menu::{Menu, MenuItem},
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    };

    let open_item = MenuItem::with_id(app, "open", "FlowLens 열기", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "완전히 종료", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_item, &quit_item])?;

    let _tray = TrayIconBuilder::with_id("flowlens-tray")
        .tooltip("FlowLens · 로컬 활동 추적 실행 중")
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "quit" => {
                let state = app.state::<SharedState>();
                let path = app.state::<SharedPath>();
                let _ = persist_before_exit(&state, &path);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
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
        schema_version: 3, date: now.date_naive().to_string(), focus_minutes, idle_minutes,
        active_ratio: if total_minutes > 0.0 { focus_minutes / total_minutes } else { 0.0 },
        app_count: state.apps.len() as u64, context_switches: state.context_switches,
        switches_per_active_hour: if focus_minutes > 0.0 { state.context_switches as f64 / (focus_minutes / 60.0) } else { 0.0 },
        average_session_minutes: if session_count > 0 { session_seconds as f64 / 60.0 / session_count as f64 } else { 0.0 },
        session_count,
        keyboard_events_per_active_minute: if focus_minutes > 0.0 { state.keyboard_actions as f64 / focus_minutes } else { 0.0 },
        mouse_distance_per_active_minute: if focus_minutes > 0.0 { state.mouse_distance_px / focus_minutes } else { 0.0 },
        mouse_clicks_per_active_minute: if focus_minutes > 0.0 { state.mouse_clicks as f64 / focus_minutes } else { 0.0 },
        mouse_wheel_notches_per_active_minute: if focus_minutes > 0.0 { state.mouse_wheel_notches as f64 / focus_minutes } else { 0.0 },
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
    vector[24] = clamp(feature.mouse_wheel_notches_per_active_minute, 100.0);
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
    EmbeddingRecord { schema_version: 2, date: feature.date.clone(), created_at: now.to_rfc3339(), dimensions: EMBEDDING_DIMENSIONS, embedding: build_embedding(feature), data_confidence: confidence(feature), feature: feature.clone() }
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
    state.mouse_distance_px = 0.0; state.mouse_clicks = 0; state.mouse_wheel_notches = 0; state.avg_click_interval_ms = 0; state.click_interval_count = 0;
    state.click_interval_mean_ms = 0.0; state.click_interval_m2 = 0.0; state.network_rx_bytes = 0; state.network_tx_bytes = 0;
    state.notification_count = 0; state.idle_seconds = 0; state.context_switches = 0; state.sessions.clear();
    state.current_session_started_at = None; state.current_session_active_seconds = 0; state.current_session_idle_seconds = 0;
    state.current_session_switches = 0; state.current_session_apps.clear(); state.last_input_at = None; state.last_input_age_seconds = 0;
    state.resume_latency_seconds = 0; state.daily_feature = DailyFeatureVector { schema_version: 3, local_only: true, ..Default::default() };
    state.reentry_episodes.clear(); state.pending_reentry = None;
    state.report_settings.work_mode_tag = None; state.report_settings.flow_satisfaction = None;
}

fn rollover_if_needed(state: &mut TrackingState, now: chrono::DateTime<chrono::Local>) {
    let today = now.date_naive().to_string();
    if state.collection_day.is_empty() {
        state.collection_day = if state.daily_feature.date.is_empty() { today.clone() } else { state.daily_feature.date.clone() };
    }
    if state.collection_day != today {
        let previous_day = state.collection_day.clone();
        if state.current_session_started_at.is_some() { close_current_session(state, now.to_rfc3339(), "day_rollover"); }
        state.pending_reentry = None;
        refresh_reentry_episodes(state, now);
        update_daily_feature(state, now);
        state.daily_feature.date = previous_day.clone();
        if state.report_settings.enabled {
            let report = build_daily_report(state, &previous_day, "finalized", now);
            upsert_daily_report(state, report);
        }
        upsert_current_embedding(state, now);
        reset_daily_activity(state);
        state.collection_day = today.clone();
        state.daily_feature.date = today;
    }
}

fn insight(level: &str, title: &str, detail: String) -> PatternInsight { PatternInsight { level: level.into(), title: title.into(), detail } }

fn local_workday_evaluation(feature: &DailyFeatureVector, prior: &[&EmbeddingRecord], baseline_similarity: Option<f64>) -> Vec<String> {
    if feature.focus_minutes < 15.0 { return Vec::new(); }
    let mut lines = vec![format!(
        "오늘은 집중 {:.0}분과 활성 비율 {:.0}%가 로컬 집계되었습니다.",
        feature.focus_minutes,
        feature.active_ratio * 100.0,
    )];
    lines.push(format!(
        "{}개 세션의 평균 길이는 {:.1}분이며, 활성 1시간당 전환은 {:.1}회입니다.",
        feature.session_count,
        feature.average_session_minutes,
        feature.switches_per_active_hour,
    ));
    if prior.is_empty() {
        lines.push("비교 기준선은 이전 업무일이 축적되면 생성됩니다. 현재 평가는 오늘의 수치형 흐름만 반영합니다.".into());
    } else {
        let average_focus = prior.iter().map(|item| item.feature.focus_minutes).sum::<f64>() / prior.len() as f64;
        let average_switches = prior.iter().map(|item| item.feature.switches_per_active_hour).sum::<f64>() / prior.len() as f64;
        let focus_delta = feature.focus_minutes - average_focus;
        let switch_delta = feature.switches_per_active_hour - average_switches;
        let comparison = if focus_delta >= 10.0 {
            format!("기준선보다 집중 시간이 {:.0}분 길고", focus_delta)
        } else if focus_delta <= -10.0 {
            format!("기준선보다 집중 시간이 {:.0}분 짧고", focus_delta.abs())
        } else {
            "기준선과 집중 시간이 비슷하고".into()
        };
        let switch_note = if switch_delta >= 2.0 {
            format!(" 전환 밀도는 {:.1}회/h 높습니다.", switch_delta)
        } else if switch_delta <= -2.0 {
            format!(" 전환 밀도는 {:.1}회/h 낮습니다.", switch_delta.abs())
        } else {
            " 전환 밀도도 큰 차이가 없습니다.".into()
        };
        let similarity_note = baseline_similarity.map(|value| format!(" 패턴 유사도는 {:.0}%입니다.", value * 100.0)).unwrap_or_default();
        lines.push(format!("{}{}{}", comparison, switch_note, similarity_note));
    }
    lines
}

fn analyze_embedding(state: &TrackingState) -> EmbeddingAnalysis {
    if !state.embedding_enabled {
        return EmbeddingAnalysis { enabled: false, local_only: true, dimensions: EMBEDDING_DIMENSIONS, history_days: state.embedding_history.len(), notice: "임베딩 분석이 꺼져 있습니다. 활성화하면 수치형 특징 25개만 이 기기에 저장해 유사 업무일과 기준선을 비교합니다.".into(), ..Default::default() };
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
    let last_workday_evaluation = local_workday_evaluation(&current.feature, &prior, baseline_similarity);
    EmbeddingAnalysis { enabled: true, local_only: true, dimensions: EMBEDDING_DIMENSIONS, history_days: state.embedding_history.len(), current: Some(current), baseline_similarity, similar_days: similar, insights, last_workday_evaluation, notice: if prior.is_empty() { "첫 번째 로컬 임베딩입니다. 하루 이상 기록하면 유사 업무일과 개인 기준선을 비교할 수 있습니다.".into() } else { "분석은 이 기기에서 계산되며, 창 제목·키 입력·URL·화면 내용은 임베딩에 포함되지 않습니다.".into() } }
}

#[tauri::command]
fn set_tracking(enabled: bool, state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<TrackingState, String> {
    let mut current = state.lock().map_err(|_| "state unavailable")?;
    if current.enabled && !enabled && current.current_session_started_at.is_some() {
        let now = chrono::Local::now();
        let app = current.active_window.clone();
        record_event(&mut current, now.to_rfc3339(), app.clone(), app, "session_end_paused");
        close_current_session(&mut current, now.to_rfc3339(), "tracking_paused");
        current.pending_reentry = None;
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
fn get_daily_report(date: String, include_draft: bool, state: tauri::State<'_, SharedState>) -> Result<DailyReportRecord, String> {
    let mut current = state.lock().map_err(|_| "state unavailable")?;
    if !current.report_settings.enabled { return Err("daily reports are disabled; enable local daily reports before requesting an archive".into()); }
    let selected_date = if date.is_empty() { current.collection_day.clone() } else { date };
    if selected_date == current.collection_day {
        if !include_draft { return Err("the current local daily report is a draft; request include_draft to view it".into()); }
        let now = chrono::Local::now();
        refresh_reentry_episodes(&mut current, now);
        return Ok(build_daily_report(&current, &selected_date, "draft", now));
    }
    current.daily_reports.iter().find(|report| report.date == selected_date && (include_draft || report.status == "finalized")).cloned().ok_or_else(|| "daily report not found in the local archive".into())
}

#[tauri::command]
fn get_daily_report_history(limit: u16, state: tauri::State<'_, SharedState>) -> Result<Vec<DailyReportListItem>, String> {
    let current = state.lock().map_err(|_| "state unavailable")?;
    let cap = if limit == 0 { 30 } else { limit.min(365) } as usize;
    Ok(current.daily_reports.iter().rev().take(cap).map(|report| DailyReportListItem {
        date: report.date.clone(), status: report.status.clone(), headline: report.report.headline.clone(),
        data_quality_level: report.data_quality.level.clone(), signal_count: report.strain.signal_count,
        work_mode_tag: report.context.work_mode_tag.clone(),
    }).collect())
}

#[tauri::command]
fn set_daily_report_enabled(enabled: bool, state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<ReportSettings, String> {
    let mut current = state.lock().map_err(|_| "state unavailable")?;
    current.report_settings.enabled = enabled;
    if !enabled { current.pending_reentry = None; }
    save(&current, &storage_path(&path)?)?;
    Ok(current.report_settings.clone())
}

#[tauri::command]
fn set_daily_work_mode_tag(tag: Option<String>, state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<DailyContext, String> {
    if let Some(value) = tag.as_deref() { if !allowed_work_mode_tag(value) { return Err("unsupported work mode tag".into()); } }
    let mut current = state.lock().map_err(|_| "state unavailable")?;
    current.report_settings.work_mode_tag = tag;
    let context = current_daily_context(&current);
    save(&current, &storage_path(&path)?)?;
    Ok(context)
}

#[tauri::command]
fn set_flow_reflection(value: Option<u8>, state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<DailyContext, String> {
    if let Some(score) = value { if !(1..=5).contains(&score) { return Err("flow reflection must be an integer from 1 to 5".into()); } }
    let mut current = state.lock().map_err(|_| "state unavailable")?;
    if !current.report_settings.reflection_enabled && value.is_some() { return Err("flow reflection is disabled in local report settings".into()); }
    current.report_settings.flow_satisfaction = value;
    let context = current_daily_context(&current);
    save(&current, &storage_path(&path)?)?;
    Ok(context)
}

#[tauri::command]
fn set_flow_reflection_enabled(enabled: bool, state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<ReportSettings, String> {
    let mut current = state.lock().map_err(|_| "state unavailable")?;
    current.report_settings.reflection_enabled = enabled;
    if !enabled { current.report_settings.flow_satisfaction = None; }
    save(&current, &storage_path(&path)?)?;
    Ok(current.report_settings.clone())
}

#[tauri::command]
fn clear_daily_report_history(state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<(), String> {
    let mut current = state.lock().map_err(|_| "state unavailable")?;
    current.daily_reports.clear(); current.reentry_episodes.clear(); current.pending_reentry = None;
    current.report_settings.work_mode_tag = None; current.report_settings.flow_satisfaction = None;
    save(&current, &storage_path(&path)?)?;
    Ok(())
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
fn exit_application(app: tauri::AppHandle, state: tauri::State<'_, SharedState>, path: tauri::State<'_, SharedPath>) -> Result<(), String> {
    persist_before_exit(&state, &path)?;
    app.exit(0);
    Ok(())
}

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

fn disk_bucket_key(now: chrono::DateTime<chrono::Local>) -> String {
    use chrono::Timelike;
    let elapsed_seconds_in_hour = now.minute() * 60 + now.second();
    let bucket_start_seconds = elapsed_seconds_in_hour / DISK_SNAPSHOT_INTERVAL_SECS as u32 * DISK_SNAPSHOT_INTERVAL_SECS as u32;
    let minute = bucket_start_seconds / 60;
    format!("{}T{:02}:{:02}:00{}", now.format("%Y-%m-%d"), now.hour(), minute, now.format("%:z"))
}

#[cfg(windows)]
fn mounted_disk_usage(at: &str) -> Vec<DiskUsageSnapshot> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetLogicalDriveStringsW};

    let mut buffer = [0u16; 2048];
    let written = unsafe { GetLogicalDriveStringsW(Some(&mut buffer)) } as usize;
    if written == 0 || written >= buffer.len() { return Vec::new(); }
    buffer[..written]
        .split(|unit| *unit == 0)
        .filter(|drive| !drive.is_empty())
        .filter_map(|drive_units| {
            let drive = String::from_utf16_lossy(drive_units);
            let wide: Vec<u16> = drive.encode_utf16().chain(std::iter::once(0)).collect();
            let (mut available, mut total, mut free) = (0u64, 0u64, 0u64);
            unsafe {
                GetDiskFreeSpaceExW(
                    PCWSTR(wide.as_ptr()),
                    Some(&mut available),
                    Some(&mut total),
                    Some(&mut free),
                ).ok()?;
            }
            if total == 0 { return None; }
            Some(DiskUsageSnapshot {
                at: at.to_string(),
                drive,
                total_bytes: total,
                free_bytes: free,
                used_bytes: total.saturating_sub(free),
            })
        })
        .collect()
}

#[cfg(not(windows))]
fn mounted_disk_usage(_at: &str) -> Vec<DiskUsageSnapshot> { Vec::new() }

fn record_disk_snapshots(state: &mut TrackingState, now: chrono::DateTime<chrono::Local>) {
    let at = disk_bucket_key(now);
    let snapshots = mounted_disk_usage(&at);
    if snapshots.is_empty() { return; }
    state.disk_snapshots.retain(|item| item.at != at);
    state.disk_snapshots.extend(snapshots);
    if state.disk_snapshots.len() > MAX_DISK_SNAPSHOTS {
        let overflow = state.disk_snapshots.len() - MAX_DISK_SNAPSHOTS;
        state.disk_snapshots.drain(0..overflow);
    }
}

#[cfg(windows)]
static WHEEL_TRACKER_STATE: OnceLock<SharedState> = OnceLock::new();
#[cfg(windows)]
static WHEEL_TRACKER_INPUT_CLOCK: OnceLock<Arc<Mutex<Instant>>> = OnceLock::new();

#[cfg(windows)]
unsafe extern "system" fn wheel_hook_proc(
    code: i32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{CallNextHookEx, MSLLHOOKSTRUCT, WM_MOUSEWHEEL};
    if code >= 0 && wparam.0 as u32 == WM_MOUSEWHEEL && lparam.0 != 0 {
        let hook = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let delta = ((hook.mouseData >> 16) as u16) as i16;
        let notches = ((i32::from(delta)).unsigned_abs() as u64).saturating_add(119) / 120;
        if notches > 0 {
            let now = chrono::Local::now();
            if let Some(state) = WHEEL_TRACKER_STATE.get() {
                if let Ok(mut current) = state.lock() {
                    if current.enabled {
                        current.mouse_wheel_notches = current.mouse_wheel_notches.saturating_add(notches);
                        current.mouse_actions = current.mouse_actions.saturating_add(notches);
                        record_flow_input(&mut current, now, 0, notches.min(u32::MAX as u64) as u32, 0.0);
                        current.last_input_at = Some(now.to_rfc3339());
                        current.last_input_age_seconds = 0;
                    }
                }
            }
            if let Some(clock) = WHEEL_TRACKER_INPUT_CLOCK.get() {
                if let Ok(mut recorded) = clock.lock() { *recorded = Instant::now(); }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

#[cfg(windows)]
fn start_wheel_tracker(state: SharedState, input_clock: Arc<Mutex<Instant>>) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
        MSG, WH_MOUSE_LL,
    };
    let _ = WHEEL_TRACKER_STATE.set(state);
    let _ = WHEEL_TRACKER_INPUT_CLOCK.set(input_clock);
    thread::spawn(|| unsafe {
        let Ok(hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(wheel_hook_proc), None, 0) else { return; };
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let _ = UnhookWindowsHookEx(hook);
    });
}

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
        let mut last_disk_bucket = String::new();
        let input_clock = Arc::new(Mutex::new(Instant::now()));
        start_wheel_tracker(state.clone(), input_clock.clone());
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
                    if let Ok(mut recorded) = input_clock.lock() { *recorded = Instant::now(); }
                    current.last_input_at = Some(input_now.to_rfc3339());
                    current.last_input_age_seconds = 0;
                } else { current.last_input_age_seconds = input_clock.lock().map(|recorded| recorded.elapsed().as_secs()).unwrap_or(0); }
            }

            if last_foreground_sample.elapsed() >= Duration::from_millis(FOREGROUND_SAMPLE_MS) {
                let elapsed_ms = last_foreground_sample.elapsed().as_millis().min(1_000) as u64;
                let now = Local::now();
                let idle = input_clock.lock().map(|recorded| recorded.elapsed().as_secs()).unwrap_or(0) >= IDLE_THRESHOLD_SECS;
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
                let idle = input_clock.lock().map(|recorded| recorded.elapsed().as_secs()).unwrap_or(0) >= IDLE_THRESHOLD_SECS;
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
                        begin_reentry_episode(&mut current, now);
                        record_event(&mut current, now.to_rfc3339(), active_app.clone(), active_app.clone(), "session_start");
                    }
                    if idle && current.current_session_started_at.is_some() {
                        record_event(&mut current, now.to_rfc3339(), active_app.clone(), active_app.clone(), "session_end_idle");
                        close_current_session(&mut current, now.to_rfc3339(), "idle");
                        begin_pending_reentry(&mut current, now);
                    }
                    refresh_reentry_episodes(&mut current, now);
                    update_daily_feature(&mut current, now);
                }
                last_second = Instant::now();
            }

            let now = Local::now();
            let disk_bucket = disk_bucket_key(now);
            if disk_bucket != last_disk_bucket {
                if let Ok(mut current) = state.lock() {
                    rollover_if_needed(&mut current, now);
                    record_disk_snapshots(&mut current, now);
                }
                last_disk_bucket = disk_bucket;
            }
            if now.minute() != last_minute {
                let network = network_totals();
                let received_delta = network.0.saturating_sub(previous_network.0);
                let sent_delta = network.1.saturating_sub(previous_network.1);
                previous_network = network;
                if let Ok(mut current) = state.lock() {
                    current.active_app_count = visible_app_count();
                    current.network_rx_bytes = current.network_rx_bytes.saturating_add(received_delta);
                    current.network_tx_bytes = current.network_tx_bytes.saturating_add(sent_delta);
                    let snapshot = MinuteSnapshot { at: now.to_rfc3339(), active_app_count: current.active_app_count, focus_seconds: current.focus_seconds, keyboard_actions: current.keyboard_actions, mouse_actions: current.mouse_actions, mouse_distance_px: current.mouse_distance_px, mouse_wheel_notches: current.mouse_wheel_notches, network_rx_bytes: current.network_rx_bytes, network_tx_bytes: current.network_tx_bytes, idle_seconds: current.idle_seconds, context_switches: current.context_switches };
                    current.minute_snapshots.push(snapshot);
                    if current.minute_snapshots.len() > MAX_MINUTE_SNAPSHOTS { current.minute_snapshots.remove(0); }
                    refresh_reentry_episodes(&mut current, now);
                    update_daily_feature(&mut current, now);
                    upsert_current_embedding(&mut current, now);
                    if current.report_settings.enabled && now.minute() % FOCUS_BUCKET_MINUTES as u32 == 0 {
                        let date = current.collection_day.clone();
                        let report = build_daily_report(&current, &date, "draft", now);
                        upsert_daily_report(&mut current, report);
                    }
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
    let state = Arc::new(Mutex::new(TrackingState::default()));
    let path = Arc::new(Mutex::new(PathBuf::new()));
    let setup_state = state.clone();
    let setup_path = path.clone();
    tauri::Builder::default()
        .on_window_event(|window, event| {
            if window.label() != "main" { return; }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(move |app| {
            let file = app.path().app_data_dir().map_err(|e| e.to_string())?.join("activity.json");
            if let Ok(mut current) = setup_state.lock() {
                *current = load(&file);
                migrate_local_state(&mut current);
                let now = chrono::Local::now();
                rollover_if_needed(&mut current, now);
                if current.collection_day.is_empty() {
                    current.collection_day = now.date_naive().to_string();
                    current.daily_feature.date = current.collection_day.clone();
                }
            }
            if let Ok(mut current_path) = setup_path.lock() { *current_path = file; }
            build_system_tray(app.handle())?;
            start_tracker(setup_state.clone(), setup_path.clone());
            Ok(())
        })
        .manage(state)
        .manage(path)
        .invoke_handler(tauri::generate_handler![set_tracking, get_tracking_state, get_focus_experience_view, get_daily_report, get_daily_report_history, set_daily_report_enabled, set_daily_work_mode_tag, set_flow_reflection, set_flow_reflection_enabled, clear_daily_report_history, get_daily_feature_vector, get_embedding_analysis, set_embedding_enabled, clear_embedding_data, clear_all_data, exit_application, request_notification_access, request_per_app_network_collection])
        .run(tauri::generate_context!())
        .expect("error while running FlowLens");
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


#[cfg(test)]
mod daily_report_tests {
    use super::*;

    fn report_minute(at: &str, focus: u16, idle: u16, switches: u16, input: u32) -> FlowMinute {
        FlowMinute { start_at: at.into(), observed_seconds: focus.saturating_add(idle), focus_seconds: focus, idle_seconds: idle, switch_count: switches, keyboard_actions: input, mouse_actions: 0, mouse_distance_px: 0.0 }
    }

    fn add_sufficient_flow(state: &mut TrackingState, date: &str, hour: u32, switches: u16, input: u32) {
        for minute in 0..120u32 {
            let total = hour * 60 + minute;
            let hour_value = total / 60;
            let minute_value = total % 60;
            state.flow_minutes.push(report_minute(&format!("{date}T{hour_value:02}:{minute_value:02}:00+09:00"), 60, 0, switches, input));
        }
        state.sessions.push(WorkSession { started_at: format!("{date}T{hour:02}:00:00+09:00"), ended_at: Some(format!("{date}T{:02}:00:00+09:00", hour + 1)), active_seconds: 3_600, idle_seconds: 0, switch_count: switches as u64 * 60, app_count: 2, end_reason: Some("idle".into()) });
        state.sessions.push(WorkSession { started_at: format!("{date}T{:02}:00:00+09:00", hour + 1), ended_at: Some(format!("{date}T{:02}:00:00+09:00", hour + 2)), active_seconds: 3_600, idle_seconds: 0, switch_count: switches as u64 * 60, app_count: 2, end_reason: Some("idle".into()) });
    }

    fn baseline_record(date: &str, tag: Option<&str>, afternoon_switches: f32, input_cv: f32, long_share: f32) -> DailyReportRecord {
        DailyReportRecord {
            schema_version: 1, date: date.into(), generated_at: format!("{date}T23:59:00+09:00"), status: "finalized".into(),
            data_quality: ReportDataQuality { level: "sufficient".into(), ..Default::default() },
            context: DailyContext { work_mode_tag: tag.map(str::to_string), ..Default::default() },
            metrics: DailyFlowMetrics { switches_per_active_hour: 4.0, input_density_cv: Some(input_cv), long_form_focus_share: long_share, ..Default::default() },
            day_parts: vec![DayPartMetrics { part: "morning".into(), observed_seconds: 3600, switches_per_active_hour: 3.0, ..Default::default() }, DayPartMetrics { part: "midday".into(), ..Default::default() }, DayPartMetrics { part: "afternoon".into(), observed_seconds: 3600, switches_per_active_hour: afternoon_switches, ..Default::default() }, DayPartMetrics { part: "evening".into(), ..Default::default() }],
            reentry: ReentryMetrics { analyzable_episode_count: 2, median_stabilization_seconds: Some(300), post_resume_switches_per_episode: Some(1.0), stabilization_missing_share: Some(0.0), ..Default::default() },
            strain: FlowStrainAssessment::default(), report: DailyReportContent::default(),
        }
    }

    #[test]
    fn finalizes_and_retains_at_most_365_local_reports() {
        let mut state = TrackingState::default();
        let start = chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        for offset in 0..=365i64 {
            let date = (start + chrono::Duration::days(offset)).to_string();
            upsert_daily_report(&mut state, DailyReportRecord { date, status: "finalized".into(), ..Default::default() });
        }
        assert_eq!(state.daily_reports.len(), MAX_DAILY_REPORTS);
        assert_eq!(state.daily_reports.first().unwrap().date, "2025-01-02");
    }

    #[test]
    fn selects_tag_and_weekday_baseline_before_broader_cohorts() {
        let mut state = TrackingState::default();
        state.report_settings.work_mode_tag = Some("구현".into());
        for day in ["2026-01-05", "2026-01-12", "2026-01-19", "2026-01-26", "2026-02-02", "2026-02-09", "2026-02-16", "2026-02-23"] {
            state.daily_reports.push(baseline_record(day, Some("구현"), 4.0, 0.2, 0.4));
        }
        let baseline = select_baseline(&state.daily_reports, &state.report_settings, "2026-03-02");
        assert_eq!(baseline.descriptor.cohort, "tag_weekday");
        assert_eq!(baseline.descriptor.sample_count, 8);
    }

    #[test]
    fn identifies_late_fragmentation_without_creating_a_health_score() {
        let mut state = TrackingState::default();
        state.report_settings.enabled = true;
        state.report_settings.work_mode_tag = Some("구현".into());
        state.collection_day = "2026-03-02".into();
        for week in 1..=14u32 {
            state.daily_reports.push(baseline_record(&format!("2026-02-{week:02}"), Some("구현"), 2.0, 0.2, 0.4));
        }
        add_sufficient_flow(&mut state, "2026-03-02", 9, 0, 8);
        add_sufficient_flow(&mut state, "2026-03-02", 11, 0, 8);
        add_sufficient_flow(&mut state, "2026-03-02", 14, 8, 40);
        let report = build_daily_report(&state, "2026-03-02", "draft", chrono::Local::now());
        let signal = report.strain.signals.iter().find(|item| item.code == "late_fragmentation").unwrap();
        assert!(matches!(signal.state.as_str(), "elevated" | "high"));
        assert!(report.report.headline.contains("변화 신호"));
    }

    #[test]
    fn adds_wheel_density_as_the_twenty_fifth_local_embedding_dimension() {
        let feature = DailyFeatureVector { mouse_wheel_notches_per_active_minute: 20.0, ..Default::default() };
        let vector = build_embedding(&feature);
        assert_eq!(vector.len(), EMBEDDING_DIMENSIONS);
        assert_eq!(vector[24], 0.2);
    }

    #[test]
    fn produces_three_line_local_workday_evaluation_with_sufficient_activity() {
        let feature = DailyFeatureVector {
            focus_minutes: 90.0, active_ratio: 0.75, session_count: 3,
            average_session_minutes: 30.0, switches_per_active_hour: 4.0,
            ..Default::default()
        };
        let prior = EmbeddingRecord {
            dimensions: EMBEDDING_DIMENSIONS,
            embedding: vec![0.0; EMBEDDING_DIMENSIONS],
            feature: DailyFeatureVector { focus_minutes: 70.0, switches_per_active_hour: 6.0, ..Default::default() },
            ..Default::default()
        };
        let lines = local_workday_evaluation(&feature, &[&prior], Some(0.88));
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| !line.is_empty()));
    }

    #[test]
    fn aligns_disk_usage_snapshots_to_thirty_minute_local_buckets() {
        use chrono::Timelike;
        let now = chrono::Local::now();
        let expected_minute = now.minute() / 30 * 30;
        let key = disk_bucket_key(now);
        assert!(key.contains(&format!("T{:02}:{expected_minute:02}:00", now.hour())));
    }

    #[test]
    fn finalizes_live_session_before_explicit_app_exit() {
        let mut state = TrackingState::default();
        let now = chrono::Local::now();
        state.collection_day = now.date_naive().to_string();
        state.current_session_started_at = Some((now - chrono::Duration::minutes(12)).to_rfc3339());
        state.current_session_active_seconds = 720;
        state.current_session_switches = 3;
        state.current_session_apps = vec!["editor.exe".into()];
        state.active_window = "editor.exe".into();
        finalize_local_state_for_exit(&mut state, now);
        assert!(state.current_session_started_at.is_none());
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].end_reason.as_deref(), Some("app_exit"));
        assert!(state.timeline.iter().any(|event| event.kind == "session_end_app_exit"));
    }

    #[test]
    fn excludes_short_and_long_breaks_from_reentry_analysis() {
        let mut state = TrackingState::default();
        state.report_settings.enabled = true;
        let now = chrono::Local::now();
        state.pending_reentry = Some(PendingReentry { break_started_at: (now - chrono::Duration::seconds(120)).to_rfc3339() });
        begin_reentry_episode(&mut state, now);
        assert!(state.reentry_episodes.is_empty());
        state.pending_reentry = Some(PendingReentry { break_started_at: (now - chrono::Duration::seconds(600)).to_rfc3339() });
        begin_reentry_episode(&mut state, now);
        assert_eq!(state.reentry_episodes.len(), 1);
        state.pending_reentry = Some(PendingReentry { break_started_at: (now - chrono::Duration::seconds(4_000)).to_rfc3339() });
        begin_reentry_episode(&mut state, now);
        assert_eq!(state.reentry_episodes.len(), 1);
    }
}
