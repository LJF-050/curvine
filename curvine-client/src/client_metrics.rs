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

use crate::file::FsContext;
use curvine_common::error::{ErrorKind, FsError};
use curvine_common::state::{MetricType, MetricValue};
use curvine_common::FsResult;
use orpc::common::{
    status_label, Counter, CounterVec, Gauge, GaugeVec, HistogramVec, Metrics, Metrics as m,
    IO_SIZE_BUCKETS_BYTES, LARGE_RPC_DURATION_BUCKETS_US, REQUEST_DURATION_BUCKETS_US,
};
use orpc::sync::FastDashMap;
use orpc::CommonResult;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};

pub struct ClientMetrics {
    pub mount_cache_hits: CounterVec,
    pub mount_cache_misses: CounterVec,
    pub last_value_map: FastDashMap<String, f64>,

    pub metadata_operation_duration: HistogramVec,
    pub write_bytes: Counter,
    pub write_time_us: Counter,
    pub read_bytes: Counter,
    pub read_time_us: Counter,
    pub block_idle_conn: Gauge,

    pub metadata_requests_total: CounterVec,
    pub metadata_errors_total: CounterVec,
    pub metadata_duration_us: HistogramVec,
    pub metadata_inflight_requests: GaugeVec,

    pub io_bytes_total: CounterVec,
    pub io_requests_total: CounterVec,
    pub io_errors_total: CounterVec,
    pub io_duration_us: HistogramVec,
    pub io_size_bytes: HistogramVec,

    pub rpc_requests_total: CounterVec,
    pub rpc_errors_total: CounterVec,
    pub rpc_duration_us: HistogramVec,
    pub rpc_inflight_requests: GaugeVec,
}

impl ClientMetrics {
    pub const PREFIX: &'static str = "client";
    pub const CURVINE_PREFIX: &'static str = "curvine_client";

    pub fn new(buckets: &[f64]) -> CommonResult<Self> {
        let cm = Self {
            mount_cache_hits: m::new_counter_vec(
                "client_mount_cache_hits",
                "mount cache miss count",
                &["id"],
            )?,
            mount_cache_misses: m::new_counter_vec(
                "client_mount_cache_misses",
                "mount cache miss count",
                &["id"],
            )?,

            last_value_map: FastDashMap::default(),

            metadata_operation_duration: m::new_histogram_vec_with_buckets(
                "client_metadata_operation_duration",
                "metadata operation duration",
                &["operation"],
                buckets,
            )?,
            write_bytes: m::new_counter("client_write_bytes", "write bytes total")?,
            write_time_us: m::new_counter("client_write_time_us", "write time us total")?,
            read_bytes: m::new_counter("client_read_bytes", "read bytes total")?,
            read_time_us: m::new_counter("client_read_time_us", "read time us total")?,
            block_idle_conn: m::new_gauge("client_block_idle_conn", "block idle conn total")?,

            metadata_requests_total: m::new_counter_vec(
                "curvine_client_metadata_requests_total",
                "client metadata requests total",
                &["operation", "status"],
            )?,
            metadata_errors_total: m::new_counter_vec(
                "curvine_client_metadata_errors_total",
                "client metadata errors total",
                &["operation", "error_kind"],
            )?,
            metadata_duration_us: m::new_histogram_vec_with_buckets(
                "curvine_client_metadata_duration_us",
                "client metadata request duration in microseconds",
                &["operation", "status"],
                REQUEST_DURATION_BUCKETS_US,
            )?,
            metadata_inflight_requests: m::new_gauge_vec(
                "curvine_client_metadata_inflight_requests",
                "client metadata requests currently in flight",
                &["operation"],
            )?,

            io_bytes_total: m::new_counter_vec(
                "curvine_client_io_bytes_total",
                "client IO bytes total",
                &["io_type", "path_type", "status"],
            )?,
            io_requests_total: m::new_counter_vec(
                "curvine_client_io_requests_total",
                "client IO requests total",
                &["io_type", "path_type", "status"],
            )?,
            io_errors_total: m::new_counter_vec(
                "curvine_client_io_errors_total",
                "client IO errors total",
                &["io_type", "path_type", "error_kind"],
            )?,
            io_duration_us: m::new_histogram_vec_with_buckets(
                "curvine_client_io_duration_us",
                "client IO duration in microseconds",
                &["io_type", "path_type", "status"],
                REQUEST_DURATION_BUCKETS_US,
            )?,
            io_size_bytes: m::new_histogram_vec_with_buckets(
                "curvine_client_io_size_bytes",
                "client IO request size in bytes",
                &["io_type", "path_type"],
                IO_SIZE_BUCKETS_BYTES,
            )?,

            rpc_requests_total: m::new_counter_vec(
                "curvine_client_rpc_requests_total",
                "client RPC requests total",
                &["target", "method", "status"],
            )?,
            rpc_errors_total: m::new_counter_vec(
                "curvine_client_rpc_errors_total",
                "client RPC errors total",
                &["target", "method", "error_kind"],
            )?,
            rpc_duration_us: m::new_histogram_vec_with_buckets(
                "curvine_client_rpc_duration_us",
                "client RPC duration in microseconds",
                &["target", "method", "status"],
                LARGE_RPC_DURATION_BUCKETS_US,
            )?,
            rpc_inflight_requests: m::new_gauge_vec(
                "curvine_client_rpc_inflight_requests",
                "client RPC requests currently in flight",
                &["target", "method"],
            )?,
        };

        Ok(cm)
    }

    pub fn error_kind_label(kind: ErrorKind) -> &'static str {
        match kind {
            ErrorKind::IO => "IO",
            ErrorKind::NotLeaderMaster => "NotLeaderMaster",
            ErrorKind::Raft => "Raft",
            ErrorKind::Timeout => "Timeout",
            ErrorKind::PBDecode => "PBDecode",
            ErrorKind::PBEncode => "PBEncode",
            ErrorKind::FileAlreadyExists => "FileAlreadyExists",
            ErrorKind::FileNotFound => "FileNotFound",
            ErrorKind::InvalidFileSize => "InvalidFileSize",
            ErrorKind::ParentNotDir => "ParentNotDir",
            ErrorKind::DirNotEmpty => "DirNotEmpty",
            ErrorKind::AbnormalData => "AbnormalData",
            ErrorKind::BlockIsWriting => "BlockIsWriting",
            ErrorKind::BlockInfo => "BlockInfo",
            ErrorKind::Lease => "Lease",
            ErrorKind::InvalidPath => "InvalidPath",
            ErrorKind::DiskOutOfSpace => "DiskOutOfSpace",
            ErrorKind::InProgress => "InProgress",
            ErrorKind::Unsupported => "Unsupported",
            ErrorKind::Ufs => "Ufs",
            ErrorKind::Expired => "Expired",
            ErrorKind::UnsupportedUfsRead => "UnsupportedUfsRead",
            ErrorKind::JobNotFound => "JobNotFound",
            ErrorKind::Pipeline => "Pipeline",
            ErrorKind::MinReplicasNotMet => "MinReplicasNotMet",
            ErrorKind::IsADirectory => "IsADirectory",
            ErrorKind::NotADirectory => "NotADirectory",
            ErrorKind::InvalidArgument => "InvalidArgument",
            ErrorKind::Common => "Common",
        }
    }

    pub fn observe_metadata<T>(&self, operation: &str, res: &FsResult<T>, duration_us: f64) {
        let status = status_label(res.is_ok());
        self.metadata_requests_total
            .with_label_values(&[operation, status])
            .inc();
        self.metadata_duration_us
            .with_label_values(&[operation, status])
            .observe(duration_us);
        if let Err(e) = res {
            self.metadata_errors_total
                .with_label_values(&[operation, Self::error_kind_label(e.kind())])
                .inc();
        }
    }

    pub fn observe_io(
        &self,
        io_type: &str,
        path_type: &str,
        status: &str,
        bytes: usize,
        duration_us: f64,
        size_bytes: Option<usize>,
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
        if let Some(size) = size_bytes {
            self.io_size_bytes
                .with_label_values(&[io_type, path_type])
                .observe(size as f64);
        }
    }

    pub fn observe_io_error(&self, io_type: &str, path_type: &str, error: &FsError) {
        self.io_errors_total
            .with_label_values(&[io_type, path_type, Self::error_kind_label(error.kind())])
            .inc();
    }

    pub fn observe_rpc<T>(&self, target: &str, method: &str, res: &FsResult<T>, duration_us: f64) {
        let status = status_label(res.is_ok());
        self.rpc_requests_total
            .with_label_values(&[target, method, status])
            .inc();
        self.rpc_duration_us
            .with_label_values(&[target, method, status])
            .observe(duration_us);
        if let Err(e) = res {
            self.rpc_errors_total
                .with_label_values(&[target, method, Self::error_kind_label(e.kind())])
                .inc();
        }
    }

    pub fn text_output(&self) -> CommonResult<String> {
        Metrics::text_output()
    }

    pub fn encode() -> CommonResult<Vec<MetricValue>> {
        let cm = FsContext::get_metrics();
        let mut metric_values = Vec::new();
        let metric_families = Metrics::registry().gather();
        for mf in metric_families {
            let name = mf.get_name().to_string();
            if !(name.starts_with(Self::PREFIX) || name.starts_with(Self::CURVINE_PREFIX)) {
                continue;
            }

            let metric_type = match mf.get_field_type() {
                prometheus::proto::MetricType::COUNTER => MetricType::Counter,
                prometheus::proto::MetricType::GAUGE => MetricType::Gauge,
                prometheus::proto::MetricType::HISTOGRAM => MetricType::Histogram,
                _ => MetricType::Gauge,
            };

            for metric in mf.get_metric() {
                let mut tags = HashMap::new();
                for label_pair in metric.get_label() {
                    tags.insert(
                        label_pair.get_name().to_string(),
                        label_pair.get_value().to_string(),
                    );
                }

                let value = match metric_type {
                    MetricType::Counter => {
                        if metric.has_counter() {
                            metric.get_counter().get_value()
                        } else {
                            0.0
                        }
                    }
                    MetricType::Gauge => {
                        if metric.has_gauge() {
                            metric.get_gauge().get_value()
                        } else {
                            0.0
                        }
                    }
                    MetricType::Histogram => {
                        if metric.has_histogram() {
                            metric.get_histogram().get_sample_count() as f64
                        } else {
                            0.0
                        }
                    }
                };

                let incr_value = {
                    let key = format!("{}:{:?}", name, tags);
                    let mut last_value = cm.last_value_map.entry(key).or_insert(0.0);
                    let incr_value = value - *last_value;
                    *last_value = value;
                    incr_value
                };

                if incr_value > 0f64 {
                    metric_values.push(MetricValue {
                        metric_type,
                        name: name.clone(),
                        value: incr_value,
                        tags,
                    });
                }
            }
        }

        Ok(metric_values)
    }
}

impl Debug for ClientMetrics {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ClientMetrics")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_kind_labels_are_bounded() {
        assert_eq!(
            ClientMetrics::error_kind_label(ErrorKind::FileNotFound),
            "FileNotFound"
        );
        assert_eq!(ClientMetrics::error_kind_label(ErrorKind::Common), "Common");
    }
    #[test]
    fn client_metrics_are_exported_with_bounded_labels() {
        let metrics = ClientMetrics::new(REQUEST_DURATION_BUCKETS_US).unwrap();

        let ok: FsResult<()> = Ok(());
        let err: FsResult<()> = Err(FsError::file_not_found("contract-test"));
        metrics.observe_metadata("GetStatus", &ok, 10.0);
        metrics.observe_metadata("Open", &err, 20.0);
        metrics.observe_io("read", "curvine", "success", 4096, 30.0, Some(4096));
        metrics.observe_io_error("write", "ufs", err.as_ref().unwrap_err());
        metrics.observe_rpc("master", "FileStatus", &ok, 40.0);
        metrics.observe_rpc("master", "OpenFile", &err, 50.0);
        metrics
            .metadata_inflight_requests
            .with_label_values(&["GetStatus"])
            .inc();
        metrics
            .metadata_inflight_requests
            .with_label_values(&["GetStatus"])
            .dec();
        metrics
            .rpc_inflight_requests
            .with_label_values(&["master", "FileStatus"])
            .inc();
        metrics
            .rpc_inflight_requests
            .with_label_values(&["master", "FileStatus"])
            .dec();

        let output = Metrics::text_output().unwrap();
        for needle in [
            "curvine_client_metadata_requests_total",
            "curvine_client_metadata_errors_total",
            "curvine_client_metadata_duration_us_bucket",
            "curvine_client_metadata_inflight_requests",
            "curvine_client_io_bytes_total",
            "curvine_client_io_requests_total",
            "curvine_client_io_errors_total",
            "curvine_client_io_duration_us_bucket",
            "curvine_client_io_size_bytes_bucket",
            "curvine_client_rpc_requests_total",
            "curvine_client_rpc_errors_total",
            "curvine_client_rpc_duration_us_bucket",
            "curvine_client_rpc_inflight_requests",
            "operation=\"GetStatus\"",
            "error_kind=\"FileNotFound\"",
            "target=\"master\"",
            "method=\"FileStatus\"",
            "path_type=\"curvine\"",
        ] {
            assert!(
                output.contains(needle),
                "missing `{}` in metrics output:\n{}",
                needle,
                output
            );
        }
    }
}
