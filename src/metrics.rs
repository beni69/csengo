use metrics::{counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

use crate::{db::DB_VERSION, GIT_REF};

pub const BUILD_INFO: &str = "csengo_build_info";

pub const PLAYBACK_TOTAL: &str = "csengo_playback_total";
pub const PLAYBACK_SECONDS: &str = "csengo_playback_seconds_total";
pub const PLAYBACK_ACTIVE: &str = "csengo_playback_active";
pub const PLAYBACK_QUEUE_SIZE: &str = "csengo_playback_queue_size";
pub const AUDIO_ERRORS: &str = "csengo_audio_device_errors_total";

pub const TASKS_CREATED: &str = "csengo_tasks_created_total";
pub const TASKS_FAILED: &str = "csengo_tasks_failed_total";
pub const TASKS_ACTIVE: &str = "csengo_tasks_active";
pub const TASK_DRIFT: &str = "csengo_task_schedule_drift_seconds";

pub const DB_OPS_TOTAL: &str = "csengo_db_operations_total";
pub const DB_OPS_DURATION: &str = "csengo_db_operation_duration_seconds";
pub const DB_FILES_COUNT: &str = "csengo_db_files_count";
pub const DB_FILES_BYTES: &str = "csengo_db_files_bytes";

pub const EMAIL_SENT: &str = "csengo_email_sent_total";

pub const HTTP_REQUESTS: &str = "csengo_http_requests_total";
pub const HTTP_DURATION: &str = "csengo_http_request_duration_seconds";

// 1ms to 1h
const DRIFT_BUCKETS: &[f64] = &[
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 5.0, 30.0, 60.0, 300.0, 900.0, 1800.0,
    3600.0,
];

// 0.1ms to 5s
const DB_BUCKETS: &[f64] = &[
    0.0001, 0.0005, 0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 5.0,
];

// 1ms to 10s
const HTTP_BUCKETS: &[f64] = &[
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// initialize the prometheus metrics exporter and describe all metrics.
/// returns a handle that can be used to render metrics.
pub fn init() -> PrometheusHandle {
    let handle = PrometheusBuilder::new()
        .set_buckets_for_metric(
            metrics_exporter_prometheus::Matcher::Full(TASK_DRIFT.to_string()),
            DRIFT_BUCKETS,
        )
        .unwrap()
        .set_buckets_for_metric(
            metrics_exporter_prometheus::Matcher::Full(DB_OPS_DURATION.to_string()),
            DB_BUCKETS,
        )
        .unwrap()
        .set_buckets_for_metric(
            metrics_exporter_prometheus::Matcher::Full(HTTP_DURATION.to_string()),
            HTTP_BUCKETS,
        )
        .unwrap()
        .install_recorder()
        .expect("failed to install Prometheus recorder");

    // describe all metrics
    describe_gauge!(
        BUILD_INFO,
        "Build information with git_ref and db_version labels"
    );

    describe_counter!(PLAYBACK_TOTAL, "Total number of playback attempts");
    describe_counter!(PLAYBACK_SECONDS, "Total seconds of audio played");
    describe_gauge!(
        PLAYBACK_ACTIVE,
        "Whether audio is currently playing (1) or not (0)"
    );
    describe_gauge!(
        PLAYBACK_QUEUE_SIZE,
        "Number of tracks in the playback queue"
    );

    describe_counter!(TASKS_CREATED, "Total number of tasks created");
    describe_counter!(TASKS_FAILED, "Total number of failed task executions");
    describe_gauge!(
        TASKS_ACTIVE,
        "Number of currently active scheduled/recurring tasks"
    );
    describe_histogram!(
        TASK_DRIFT,
        "Difference between scheduled and actual execution time in seconds"
    );

    describe_counter!(DB_OPS_TOTAL, "Total number of database operations");
    describe_histogram!(
        DB_OPS_DURATION,
        "Duration of database operations in seconds"
    );
    describe_gauge!(
        DB_FILES_COUNT,
        "Number of audio files stored in the database"
    );
    describe_gauge!(
        DB_FILES_BYTES,
        "Total size of audio files stored in the database in bytes"
    );

    describe_counter!(EMAIL_SENT, "Total number of emails sent");

    describe_counter!(HTTP_REQUESTS, "Total number of HTTP requests");
    describe_histogram!(HTTP_DURATION, "Duration of HTTP requests in seconds");

    describe_counter!(AUDIO_ERRORS, "Total number of audio device errors");

    gauge!(BUILD_INFO, "git_ref" => GIT_REF, "db_version" => DB_VERSION.to_string()).set(1.0);

    // initialize gauges to 0
    gauge!(PLAYBACK_ACTIVE).set(0.0);
    gauge!(PLAYBACK_QUEUE_SIZE).set(0.0);
    gauge!(TASKS_ACTIVE, "type" => "scheduled").set(0.0);
    gauge!(TASKS_ACTIVE, "type" => "recurring").set(0.0);
    gauge!(DB_FILES_COUNT).set(0.0);
    gauge!(DB_FILES_BYTES).set(0.0);

    handle
}

pub fn record_playback_success(task_type: &str, task_name: &str) {
    counter!(PLAYBACK_TOTAL, "status" => "success", "task_type" => task_type.to_string(), "task_name" => task_name.to_string()).increment(1);
}

pub fn record_playback_failure(task_type: &str, task_name: &str) {
    counter!(PLAYBACK_TOTAL, "status" => "error", "task_type" => task_type.to_string(), "task_name" => task_name.to_string()).increment(1);
    counter!(TASKS_FAILED, "task_type" => task_type.to_string(), "task_name" => task_name.to_string()).increment(1);
}

pub fn record_playback_seconds(task_name: &str, seconds: f64) {
    counter!(PLAYBACK_SECONDS, "task_name" => task_name.to_string()).increment(seconds as u64);
}

pub fn set_playback_active(active: bool) {
    gauge!(PLAYBACK_ACTIVE).set(if active { 1.0 } else { 0.0 });
}

pub fn set_queue_size(size: usize) {
    gauge!(PLAYBACK_QUEUE_SIZE).set(size as f64);
}

pub fn record_task_created(task_type: &str) {
    counter!(TASKS_CREATED, "type" => task_type.to_string()).increment(1);
}

pub fn inc_active_tasks(task_type: &str) {
    gauge!(TASKS_ACTIVE, "type" => task_type.to_string()).increment(1.0);
}

pub fn dec_active_tasks(task_type: &str) {
    gauge!(TASKS_ACTIVE, "type" => task_type.to_string()).decrement(1.0);
}

pub fn record_drift(task_type: &str, task_name: &str, drift_seconds: f64) {
    histogram!(TASK_DRIFT, "task_type" => task_type.to_string(), "task_name" => task_name.to_string()).record(drift_seconds);
}

pub fn record_db_operation(operation: &str, table: &str, duration_seconds: f64) {
    counter!(DB_OPS_TOTAL, "operation" => operation.to_string(), "table" => table.to_string())
        .increment(1);
    histogram!(DB_OPS_DURATION, "operation" => operation.to_string(), "table" => table.to_string())
        .record(duration_seconds);
}

pub fn set_file_stats(count: i64, bytes: i64) {
    gauge!(DB_FILES_COUNT).set(count as f64);
    gauge!(DB_FILES_BYTES).set(bytes as f64);
}

pub fn record_email(success: bool) {
    counter!(EMAIL_SENT, "status" => if success { "success" } else { "error" }).increment(1);
}

pub fn record_http_request(method: &str, path: &str, status: u16, duration_seconds: f64) {
    let normalized_path = normalize_path(path);
    counter!(HTTP_REQUESTS, "method" => method.to_string(), "path" => normalized_path.clone(), "status" => status.to_string()).increment(1);
    histogram!(HTTP_DURATION, "method" => method.to_string(), "path" => normalized_path)
        .record(duration_seconds);
}

pub fn record_audio_error() {
    counter!(AUDIO_ERRORS).increment(1);
}

// normalize HTTP paths to avoid high cardinality
fn normalize_path(path: &str) -> String {
    if path.starts_with("/htmx/task/") && path.len() > 11 {
        return "/htmx/task/:id".to_string();
    }
    if path.starts_with("/htmx/file/") && path.len() > 11 {
        return "/htmx/file/:fname".to_string();
    }
    if path.starts_with("/api/file/") && path.len() > 10 {
        return "/api/file/:fname".to_string();
    }
    if path.starts_with("/static/") {
        return "/static/*path".to_string();
    }
    path.to_string()
}
