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

use crate::common::Utils;
use std::time::{Duration, Instant};

pub const REQUEST_DURATION_BUCKETS_US: &[f64] = &[
    10.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0, 25000.0, 50000.0, 100000.0,
    250000.0, 500000.0, 1000000.0, 2500000.0, 5000000.0, 10000000.0,
];

pub const STAGE_DURATION_BUCKETS_US: &[f64] = &[
    5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0, 25000.0, 50000.0,
    100000.0,
];

pub const LARGE_RPC_DURATION_BUCKETS_US: &[f64] = &[
    50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0, 25000.0, 50000.0, 100000.0,
    250000.0, 500000.0, 1000000.0, 2500000.0, 5000000.0, 10000000.0, 30000000.0,
];

pub const IO_SIZE_BUCKETS_BYTES: &[f64] = &[
    4096.0,
    16384.0,
    65536.0,
    262144.0,
    1048576.0,
    4194304.0,
    16777216.0,
    67108864.0,
    268435456.0,
];

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RequestSource {
    Fuse,
    Cli,
    Web,
    Internal,
    Worker,
    Unknown,
}

impl RequestSource {
    pub fn as_label(self) -> &'static str {
        match self {
            RequestSource::Fuse => "fuse",
            RequestSource::Cli => "cli",
            RequestSource::Web => "web",
            RequestSource::Internal => "internal",
            RequestSource::Worker => "worker",
            RequestSource::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct RequestContext {
    pub trace_id: u128,
    pub parent_id: u64,
    pub span_id: u64,
    pub source: RequestSource,
    pub sampled: bool,
}

impl RequestContext {
    pub fn new_root(source: RequestSource) -> Self {
        let high = Utils::req_id() as u64 as u128;
        let low = Utils::req_id() as u64 as u128;
        Self {
            trace_id: (high << 64) | low,
            parent_id: 0,
            span_id: Utils::req_id() as u64,
            source,
            sampled: false,
        }
    }

    pub fn child(self) -> Self {
        Self {
            trace_id: self.trace_id,
            parent_id: self.span_id,
            span_id: Utils::req_id() as u64,
            source: self.source,
            sampled: self.sampled,
        }
    }

    pub fn sampled(mut self, sampled: bool) -> Self {
        self.sampled = sampled;
        self
    }

    pub fn trace_id_hex(&self) -> String {
        format!("{:032x}", self.trace_id)
    }
}

pub fn duration_us(duration: Duration) -> f64 {
    duration.as_micros() as f64
}

pub fn elapsed_us(start: Instant) -> f64 {
    duration_us(start.elapsed())
}

pub fn status_label(ok: bool) -> &'static str {
    if ok {
        "success"
    } else {
        "error"
    }
}

pub fn errno_label(errno: i32) -> &'static str {
    match errno {
        libc::EACCES => "EACCES",
        libc::EAGAIN => "EAGAIN",
        libc::EBUSY => "EBUSY",
        libc::EEXIST => "EEXIST",
        libc::EINTR => "EINTR",
        libc::EINVAL => "EINVAL",
        libc::EIO => "EIO",
        libc::EISDIR => "EISDIR",
        libc::ENODEV => "ENODEV",
        libc::ENOENT => "ENOENT",
        libc::ENOSPC => "ENOSPC",
        libc::ENOSYS => "ENOSYS",
        libc::ENOTDIR => "ENOTDIR",
        libc::ENOTEMPTY => "ENOTEMPTY",
        libc::EOPNOTSUPP => "EOPNOTSUPP",
        libc::ETIMEDOUT => "ETIMEDOUT",
        _ => "UNKNOWN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_context_child_keeps_trace() {
        let root = RequestContext::new_root(RequestSource::Fuse).sampled(true);
        let child = root.child();
        assert_eq!(root.trace_id, child.trace_id);
        assert_eq!(child.parent_id, root.span_id);
        assert_ne!(child.span_id, root.span_id);
        assert!(child.sampled);
        assert_eq!(child.source.as_label(), "fuse");
        assert_eq!(root.trace_id_hex().len(), 32);
    }

    #[test]
    fn labels_are_bounded() {
        assert_eq!(status_label(true), "success");
        assert_eq!(status_label(false), "error");
        assert_eq!(errno_label(libc::ENOENT), "ENOENT");
        assert_eq!(errno_label(-12345), "UNKNOWN");
    }
}
