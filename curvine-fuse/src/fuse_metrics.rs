// Copyright 2025 OPPO.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use once_cell::sync::OnceCell;

use orpc::common::{
    elapsed_us, CounterVec, Gauge, GaugeVec, Histogram, HistogramVec, Metrics as m,
    IO_SIZE_BUCKETS_BYTES, REQUEST_DURATION_BUCKETS_US, STAGE_DURATION_BUCKETS_US,
};
use orpc::CommonResult;
use std::time::Instant;

static FUSE_METRICS: OnceCell<FuseMetrics> = OnceCell::new();

#[derive(Clone)]
pub struct FuseReplyMetrics {
    pub opcode: &'static str,
    pub kind: &'static str,
    pub status: &'static str,
    pub errno: Option<&'static str>,
    pub request_start: Instant,
    pub operation_duration_us: f64,
}

impl FuseReplyMetrics {
    pub fn as_error(&self, errno: &'static str) -> Self {
        Self {
            status: "error",
            errno: Some(errno),
            ..self.clone()
        }
    }
}

pub struct FuseMetrics {
    pub inode_num: Gauge,
    pub file_handle_num: Gauge,
    pub dir_handle_num: Gauge,
    pub fuse_used_memory_bytes: Gauge,

    pub write_back_active_inode_num: Gauge,
    pub write_back_mem_usage: Gauge,
    pub write_back_mem_limit: Gauge,

    pub inode_count: Gauge,
    pub file_handle_count: Gauge,
    pub dir_handle_count: Gauge,
    pub used_memory_bytes: Gauge,
    pub write_back_active_inode_count: Gauge,
    pub write_back_memory_usage_bytes: Gauge,
    pub write_back_memory_limit_bytes: Gauge,

    pub requests_total: CounterVec,
    pub errors_total: CounterVec,
    pub interrupted_total: CounterVec,
    pub unsupported_total: CounterVec,
    pub request_duration_us: HistogramVec,
    pub operation_duration_us: HistogramVec,
    pub stage_duration_us: HistogramVec,
    pub inflight_requests: GaugeVec,
    pub pending_interruptible_requests: Gauge,
    pub reply_queue_depth: Gauge,
    pub receiver_tasks: GaugeVec,
    pub sender_tasks: GaugeVec,
    pub metrics_scrape_duration_us: Histogram,
    pub metrics_scrape_bytes: Gauge,
    pub metrics_series_estimate: Gauge,

    pub io_bytes_total: CounterVec,
    pub io_requests_total: CounterVec,
    pub io_errors_total: CounterVec,
    pub io_duration_us: HistogramVec,
    pub io_size_bytes: HistogramVec,
    pub stream_queue_depth: GaugeVec,
    pub stream_enqueue_duration_us: HistogramVec,
    pub node_cache_hits_total: CounterVec,
    pub node_cache_misses_total: CounterVec,
    pub node_cache_invalidations_total: CounterVec,
    pub readdir_entries_total: CounterVec,
    pub readdir_duration_us: HistogramVec,
    pub state_persist_total: CounterVec,
    pub state_persist_duration_us: HistogramVec,
    pub state_restore_total: CounterVec,
    pub state_restore_duration_us: HistogramVec,
}

impl FuseMetrics {
    pub fn ensure_init() -> CommonResult<()> {
        FUSE_METRICS.get_or_try_init(Self::new)?;
        Ok(())
    }

    pub fn get() -> &'static Self {
        FUSE_METRICS
            .get()
            .expect("FuseMetrics not initialized; call ensure_init from CurvineFileSystem::new")
    }

    fn new() -> CommonResult<Self> {
        Ok(Self {
            inode_num: m::new_gauge("inode_num", "FUSE inode count in dcache")?,
            file_handle_num: m::new_gauge("file_handle_num", "FUSE open file handle count")?,
            dir_handle_num: m::new_gauge("dir_handle_num", "FUSE open directory handle count")?,
            fuse_used_memory_bytes: m::new_gauge("fuse_used_memory_bytes", "Total memory used")?,
            write_back_active_inode_num: m::new_gauge(
                "write_back_active_inode_num",
                "FUSE write-back active inode count",
            )?,
            write_back_mem_usage: m::new_gauge(
                "write_back_mem_usage",
                "FUSE write-back page cache usage (bytes)",
            )?,
            write_back_mem_limit: m::new_gauge(
                "write_back_mem_limit",
                "FUSE write-back page cache size limit (bytes)",
            )?,

            inode_count: m::new_gauge("curvine_fuse_inode_count", "FUSE inode count in dcache")?,
            file_handle_count: m::new_gauge(
                "curvine_fuse_file_handle_count",
                "FUSE open file handle count",
            )?,
            dir_handle_count: m::new_gauge(
                "curvine_fuse_dir_handle_count",
                "FUSE open directory handle count",
            )?,
            used_memory_bytes: m::new_gauge(
                "curvine_fuse_used_memory_bytes",
                "FUSE process memory used in bytes",
            )?,
            write_back_active_inode_count: m::new_gauge(
                "curvine_fuse_write_back_active_inode_count",
                "FUSE write-back active inode count",
            )?,
            write_back_memory_usage_bytes: m::new_gauge(
                "curvine_fuse_write_back_memory_usage_bytes",
                "FUSE write-back page cache usage in bytes",
            )?,
            write_back_memory_limit_bytes: m::new_gauge(
                "curvine_fuse_write_back_memory_limit_bytes",
                "FUSE write-back page cache size limit in bytes",
            )?,

            requests_total: m::new_counter_vec(
                "curvine_fuse_requests_total",
                "FUSE requests by opcode, kind, and final status",
                &["opcode", "kind", "status"],
            )?,
            errors_total: m::new_counter_vec(
                "curvine_fuse_errors_total",
                "FUSE errors by opcode, kind, and errno",
                &["opcode", "kind", "errno"],
            )?,
            interrupted_total: m::new_counter_vec(
                "curvine_fuse_interrupted_total",
                "FUSE interrupted requests by opcode",
                &["opcode"],
            )?,
            unsupported_total: m::new_counter_vec(
                "curvine_fuse_unsupported_total",
                "FUSE unsupported requests by opcode",
                &["opcode"],
            )?,
            request_duration_us: m::new_histogram_vec_with_buckets(
                "curvine_fuse_request_duration_us",
                "End-to-end FUSE request duration in microseconds",
                &["opcode", "kind", "status"],
                REQUEST_DURATION_BUCKETS_US,
            )?,
            operation_duration_us: m::new_histogram_vec_with_buckets(
                "curvine_fuse_operation_duration_us",
                "FUSE operation duration in microseconds",
                &["opcode", "kind", "status"],
                REQUEST_DURATION_BUCKETS_US,
            )?,
            stage_duration_us: m::new_histogram_vec_with_buckets(
                "curvine_fuse_stage_duration_us",
                "FUSE framework stage duration in microseconds",
                &["stage", "kind", "status"],
                STAGE_DURATION_BUCKETS_US,
            )?,
            inflight_requests: m::new_gauge_vec(
                "curvine_fuse_inflight_requests",
                "Current FUSE in-flight requests by kind",
                &["kind"],
            )?,
            pending_interruptible_requests: m::new_gauge(
                "curvine_fuse_pending_interruptible_requests",
                "Current pending interruptible FUSE requests",
            )?,
            reply_queue_depth: m::new_gauge(
                "curvine_fuse_reply_queue_depth",
                "Approximate FUSE reply queue depth",
            )?,
            receiver_tasks: m::new_gauge_vec(
                "curvine_fuse_receiver_tasks",
                "FUSE receiver tasks by state",
                &["state"],
            )?,
            sender_tasks: m::new_gauge_vec(
                "curvine_fuse_sender_tasks",
                "FUSE sender tasks by state",
                &["state"],
            )?,
            metrics_scrape_duration_us: m::new_histogram_with_buckets(
                "curvine_fuse_metrics_scrape_duration_us",
                "FUSE metrics scrape handler duration in microseconds",
                STAGE_DURATION_BUCKETS_US,
            )?,
            metrics_scrape_bytes: m::new_gauge(
                "curvine_fuse_metrics_scrape_bytes",
                "FUSE metrics scrape response size in bytes",
            )?,
            metrics_series_estimate: m::new_gauge(
                "curvine_fuse_metrics_series_estimate",
                "Estimated FUSE metrics series count",
            )?,

            io_bytes_total: m::new_counter_vec(
                "curvine_fuse_io_bytes_total",
                "FUSE IO bytes by type, path type, and status",
                &["io_type", "path_type", "status"],
            )?,
            io_requests_total: m::new_counter_vec(
                "curvine_fuse_io_requests_total",
                "FUSE IO requests by type, path type, and status",
                &["io_type", "path_type", "status"],
            )?,
            io_errors_total: m::new_counter_vec(
                "curvine_fuse_io_errors_total",
                "FUSE IO errors by type, path type, and error kind",
                &["io_type", "path_type", "error_kind"],
            )?,
            io_duration_us: m::new_histogram_vec_with_buckets(
                "curvine_fuse_io_duration_us",
                "FUSE IO duration in microseconds",
                &["io_type", "path_type", "status"],
                REQUEST_DURATION_BUCKETS_US,
            )?,
            io_size_bytes: m::new_histogram_vec_with_buckets(
                "curvine_fuse_io_size_bytes",
                "FUSE IO request or response size in bytes",
                &["io_type", "path_type"],
                IO_SIZE_BUCKETS_BYTES,
            )?,
            stream_queue_depth: m::new_gauge_vec(
                "curvine_fuse_stream_queue_depth",
                "Approximate FUSE stream queue depth by IO type",
                &["io_type"],
            )?,
            stream_enqueue_duration_us: m::new_histogram_vec_with_buckets(
                "curvine_fuse_stream_enqueue_duration_us",
                "FUSE stream enqueue duration in microseconds",
                &["io_type", "status"],
                STAGE_DURATION_BUCKETS_US,
            )?,
            node_cache_hits_total: m::new_counter_vec(
                "curvine_fuse_node_cache_hits_total",
                "FUSE node cache hits by cache name",
                &["cache"],
            )?,
            node_cache_misses_total: m::new_counter_vec(
                "curvine_fuse_node_cache_misses_total",
                "FUSE node cache misses by cache name and reason",
                &["cache", "reason"],
            )?,
            node_cache_invalidations_total: m::new_counter_vec(
                "curvine_fuse_node_cache_invalidations_total",
                "FUSE node cache invalidations by reason",
                &["reason"],
            )?,
            readdir_entries_total: m::new_counter_vec(
                "curvine_fuse_readdir_entries_total",
                "FUSE readdir entries returned by status",
                &["status"],
            )?,
            readdir_duration_us: m::new_histogram_vec_with_buckets(
                "curvine_fuse_readdir_duration_us",
                "FUSE readdir duration in microseconds",
                &["status"],
                REQUEST_DURATION_BUCKETS_US,
            )?,
            state_persist_total: m::new_counter_vec(
                "curvine_fuse_state_persist_total",
                "FUSE state persist attempts by status",
                &["status"],
            )?,
            state_persist_duration_us: m::new_histogram_vec_with_buckets(
                "curvine_fuse_state_persist_duration_us",
                "FUSE state persist duration in microseconds",
                &["status"],
                REQUEST_DURATION_BUCKETS_US,
            )?,
            state_restore_total: m::new_counter_vec(
                "curvine_fuse_state_restore_total",
                "FUSE state restore attempts by status",
                &["status"],
            )?,
            state_restore_duration_us: m::new_histogram_vec_with_buckets(
                "curvine_fuse_state_restore_duration_us",
                "FUSE state restore duration in microseconds",
                &["status"],
                REQUEST_DURATION_BUCKETS_US,
            )?,
        })
    }

    pub fn observe_stage(
        &self,
        stage: &'static str,
        kind: &'static str,
        status: &'static str,
        duration_us: f64,
    ) {
        self.stage_duration_us
            .with_label_values(&[stage, kind, status])
            .observe(duration_us);
    }

    pub fn observe_stream_enqueue(
        &self,
        io_type: &'static str,
        status: &'static str,
        duration_us: f64,
    ) {
        self.stream_enqueue_duration_us
            .with_label_values(&[io_type, status])
            .observe(duration_us);
    }

    pub fn observe_io(
        &self,
        io_type: &'static str,
        path_type: &'static str,
        status: &'static str,
        bytes: usize,
        duration_us: f64,
        size: Option<usize>,
    ) {
        self.io_requests_total
            .with_label_values(&[io_type, path_type, status])
            .inc();
        if bytes > 0 {
            self.io_bytes_total
                .with_label_values(&[io_type, path_type, status])
                .inc_by(bytes as i64);
        }
        self.io_duration_us
            .with_label_values(&[io_type, path_type, status])
            .observe(duration_us);
        if let Some(size) = size {
            self.io_size_bytes
                .with_label_values(&[io_type, path_type])
                .observe(size as f64);
        }
    }

    pub fn observe_io_error(
        &self,
        io_type: &'static str,
        path_type: &'static str,
        error_kind: &'static str,
    ) {
        self.io_errors_total
            .with_label_values(&[io_type, path_type, error_kind])
            .inc();
    }

    pub fn observe_reply(&self, metrics: &FuseReplyMetrics, reply_write_us: f64, write_ok: bool) {
        let final_metrics = if write_ok {
            metrics.clone()
        } else {
            metrics.as_error("EIO")
        };

        self.observe_stage(
            "reply_write",
            final_metrics.kind,
            final_metrics.status,
            reply_write_us,
        );
        self.observe_request_complete(&final_metrics);
    }

    pub fn observe_no_reply(&self, metrics: &FuseReplyMetrics) {
        self.observe_request_complete(metrics);
    }

    pub fn observe_enqueue_failure(&self, metrics: &FuseReplyMetrics) {
        let final_metrics = metrics.as_error("EIO");
        self.observe_request_complete(&final_metrics);
    }

    fn observe_request_complete(&self, metrics: &FuseReplyMetrics) {
        self.requests_total
            .with_label_values(&[metrics.opcode, metrics.kind, metrics.status])
            .inc();
        if let Some(errno) = metrics.errno {
            self.errors_total
                .with_label_values(&[metrics.opcode, metrics.kind, errno])
                .inc();
        }
        self.request_duration_us
            .with_label_values(&[metrics.opcode, metrics.kind, metrics.status])
            .observe(elapsed_us(metrics.request_start));
        self.operation_duration_us
            .with_label_values(&[metrics.opcode, metrics.kind, metrics.status])
            .observe(metrics.operation_duration_us);
        self.inflight_requests
            .with_label_values(&[metrics.kind])
            .dec();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orpc::common::Metrics;

    fn assert_contains(text: &str, needle: &str) {
        assert!(
            text.contains(needle),
            "missing `{}` in metrics output:\n{}",
            needle,
            text
        );
    }

    #[test]
    fn request_metrics_are_exported_with_bounded_labels() {
        FuseMetrics::ensure_init().unwrap();
        let metrics = FuseMetrics::get();

        metrics.inode_count.set(3);
        metrics.file_handle_count.set(2);
        metrics.dir_handle_count.set(1);
        metrics.metrics_scrape_duration_us.observe(9.0);
        metrics.metrics_scrape_bytes.set(128);
        metrics.metrics_series_estimate.set(64);
        metrics.observe_stage("receive", "metadata", "success", 5.0);
        metrics.interrupted_total.with_label_values(&["Read"]).inc();
        metrics
            .unsupported_total
            .with_label_values(&["Unknown"])
            .inc();
        metrics
            .inflight_requests
            .with_label_values(&["metadata"])
            .inc();
        metrics
            .inflight_requests
            .with_label_values(&["metadata"])
            .dec();

        let success = FuseReplyMetrics {
            opcode: "Lookup",
            kind: "metadata",
            status: "success",
            errno: None,
            request_start: Instant::now(),
            operation_duration_us: 12.0,
        };
        metrics.observe_no_reply(&success);

        let error = FuseReplyMetrics {
            opcode: "GetAttr",
            kind: "metadata",
            status: "error",
            errno: Some("ENOENT"),
            request_start: Instant::now(),
            operation_duration_us: 7.0,
        };
        metrics.observe_no_reply(&error);

        let output = Metrics::text_output().unwrap();
        for needle in [
            "curvine_fuse_requests_total",
            "curvine_fuse_errors_total",
            "curvine_fuse_interrupted_total",
            "curvine_fuse_unsupported_total",
            "curvine_fuse_request_duration_us_bucket",
            "curvine_fuse_operation_duration_us_bucket",
            "curvine_fuse_stage_duration_us_bucket",
            "curvine_fuse_inflight_requests",
            "curvine_fuse_inode_count",
            "curvine_fuse_file_handle_count",
            "curvine_fuse_dir_handle_count",
            "curvine_fuse_metrics_scrape_duration_us_bucket",
            "curvine_fuse_metrics_scrape_bytes",
            "curvine_fuse_metrics_series_estimate",
            "opcode=\"Lookup\"",
            "kind=\"metadata\"",
            "status=\"success\"",
            "errno=\"ENOENT\"",
        ] {
            assert_contains(&output, needle);
        }
    }
    #[test]
    fn io_cache_and_state_metrics_are_exported_with_bounded_labels() {
        FuseMetrics::ensure_init().unwrap();
        let metrics = FuseMetrics::get();

        metrics.observe_io("read", "curvine", "success", 4096, 33.0, Some(4096));
        metrics.observe_io_error("write", "ufs", "IO");
        metrics.observe_stream_enqueue("write", "success", 4.0);
        metrics
            .stream_queue_depth
            .with_label_values(&["write"])
            .inc();
        metrics
            .node_cache_hits_total
            .with_label_values(&["node"])
            .inc();
        metrics
            .node_cache_misses_total
            .with_label_values(&["node", "not_found_or_expired"])
            .inc();
        metrics
            .node_cache_invalidations_total
            .with_label_values(&["explicit"])
            .inc();
        metrics
            .readdir_entries_total
            .with_label_values(&["success"])
            .inc_by(3);
        metrics
            .readdir_duration_us
            .with_label_values(&["success"])
            .observe(44.0);
        metrics
            .state_persist_total
            .with_label_values(&["success"])
            .inc();
        metrics
            .state_persist_duration_us
            .with_label_values(&["success"])
            .observe(55.0);
        metrics
            .state_restore_total
            .with_label_values(&["error"])
            .inc();
        metrics
            .state_restore_duration_us
            .with_label_values(&["error"])
            .observe(66.0);

        let output = Metrics::text_output().unwrap();
        for needle in [
            "curvine_fuse_io_bytes_total",
            "curvine_fuse_io_requests_total",
            "curvine_fuse_io_errors_total",
            "curvine_fuse_io_duration_us_bucket",
            "curvine_fuse_io_size_bytes_bucket",
            "curvine_fuse_stream_queue_depth",
            "curvine_fuse_stream_enqueue_duration_us_bucket",
            "curvine_fuse_node_cache_hits_total",
            "curvine_fuse_node_cache_misses_total",
            "curvine_fuse_node_cache_invalidations_total",
            "curvine_fuse_readdir_entries_total",
            "curvine_fuse_readdir_duration_us_bucket",
            "curvine_fuse_state_persist_total",
            "curvine_fuse_state_persist_duration_us_bucket",
            "curvine_fuse_state_restore_total",
            "curvine_fuse_state_restore_duration_us_bucket",
            "io_type=\"read\"",
            "path_type=\"curvine\"",
            "error_kind=\"IO\"",
            "reason=\"not_found_or_expired\"",
        ] {
            assert_contains(&output, needle);
        }
    }
}
