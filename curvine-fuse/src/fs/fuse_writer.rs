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

use crate::fs::operator::Write;
use crate::raw::fuse_abi::fuse_write_out;
use crate::session::FuseResponse;
use crate::FuseMetrics;
use curvine_client::unified::UnifiedWriter;
use curvine_common::conf::FuseConf;
use curvine_common::error::FsError;
use curvine_common::fs::{Path, Writer};
use curvine_common::state::{FileAllocOpts, FileStatus};
use curvine_common::FsResult;
use log::error;
use orpc::common::{elapsed_us, status_label};
use orpc::runtime::{RpcRuntime, Runtime};
use orpc::sync::channel::{AsyncChannel, AsyncReceiver, AsyncSender, CallChannel, CallSender};
use orpc::sync::{AtomicCounter, ErrorMonitor};
use orpc::sys::DataSlice;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio_util::bytes::Bytes;

enum WriteTask {
    Write(i64, Bytes, FuseResponse),
    Flush(CallSender<i8>, Option<FuseResponse>),
    Complete(CallSender<i8>, Option<FuseResponse>),
    Resize(CallSender<i8>, FileAllocOpts),
}

pub struct FuseWriter {
    path: Path,
    sender: AsyncSender<WriteTask>,
    err_monitor: Arc<ErrorMonitor<FsError>>,
    status: FileStatus,
    is_ufs: bool,
    len: Arc<Mutex<i64>>,
    write_ver: AtomicCounter,
}

impl FuseWriter {
    pub fn new(conf: &FuseConf, rt: Arc<Runtime>, writer: UnifiedWriter) -> Self {
        let is_ufs = !writer.path().is_cv();
        let path = writer.path().clone();
        let err_monitor = Arc::new(ErrorMonitor::new());
        let (sender, receiver) = AsyncChannel::new(conf.stream_channel_size).split();

        let status = writer.status().clone();
        let monitor = err_monitor.clone();
        let len = Arc::new(Mutex::new(status.len));
        let write_ver = AtomicCounter::new(0);

        let len1 = len.clone();
        rt.spawn(async move {
            let res = Self::writer_future(writer, receiver, len1).await;
            match res {
                Ok(_) => (),

                Err(e) => {
                    error!("fuse writer error: {}", e);
                    monitor.set_error(e);
                }
            }
        });

        Self {
            path,
            sender,
            err_monitor,
            status,
            is_ufs,
            len,
            write_ver,
        }
    }

    pub fn write_ver(&self) -> u64 {
        self.write_ver.get()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn status(&self) -> &FileStatus {
        &self.status
    }

    pub fn is_ufs(&self) -> bool {
        self.is_ufs
    }

    fn check_error(&self, e: FsError) -> FsError {
        self.err_monitor.take_error().unwrap_or(e)
    }

    fn path_type(path: &Path) -> &'static str {
        if path.is_cv() {
            "curvine"
        } else {
            "ufs"
        }
    }

    pub async fn write(&mut self, op: Write<'_>, reply: FuseResponse) -> FsResult<()> {
        self.write_ver.incr();
        let enqueue_start = Instant::now();
        let res = self
            .sender
            .send(WriteTask::Write(op.arg.offset as i64, op.data, reply))
            .await
            .map_err(|e| self.check_error(e.into()));
        let status = status_label(res.is_ok());
        FuseMetrics::get().observe_stream_enqueue("write", status, elapsed_us(enqueue_start));
        if res.is_ok() {
            FuseMetrics::get()
                .stream_queue_depth
                .with_label_values(&["write"])
                .inc();
        }
        res
    }

    pub async fn flush(&mut self, reply: Option<FuseResponse>) -> FsResult<()> {
        let (rx, tx) = CallChannel::channel();
        let enqueue_start = Instant::now();
        let send_res = self
            .sender
            .send(WriteTask::Flush(rx, reply))
            .await
            .map_err(|e| self.check_error(e.into()));
        let status = status_label(send_res.is_ok());
        FuseMetrics::get().observe_stream_enqueue("flush", status, elapsed_us(enqueue_start));
        if send_res.is_ok() {
            FuseMetrics::get()
                .stream_queue_depth
                .with_label_values(&["flush"])
                .inc();
        }
        send_res?;
        tx.receive().await.map_err(|e| self.check_error(e.into()))?;
        Ok(())
    }

    pub async fn complete(&mut self, reply: Option<FuseResponse>) -> FsResult<()> {
        let (rx, tx) = CallChannel::channel();
        let enqueue_start = Instant::now();
        let send_res = self
            .sender
            .send(WriteTask::Complete(rx, reply))
            .await
            .map_err(|e| self.check_error(e.into()));
        let status = status_label(send_res.is_ok());
        FuseMetrics::get().observe_stream_enqueue("release", status, elapsed_us(enqueue_start));
        if send_res.is_ok() {
            FuseMetrics::get()
                .stream_queue_depth
                .with_label_values(&["release"])
                .inc();
        }
        send_res?;
        tx.receive().await.map_err(|e| self.check_error(e.into()))?;
        Ok(())
    }

    pub async fn resize(&mut self, opts: FileAllocOpts) -> FsResult<()> {
        let fun = async {
            let (rx, tx) = CallChannel::channel();
            self.sender.send(WriteTask::Resize(rx, opts)).await?;
            tx.receive().await?;
            Ok::<(), FsError>(())
        };
        self.write_ver.incr();
        fun.await.map_err(|e| self.check_error(e))
    }

    pub fn len(&self) -> i64 {
        *self.len.lock().unwrap()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    async fn writer_future(
        mut writer: UnifiedWriter,
        mut req_receiver: AsyncReceiver<WriteTask>,
        file_len: Arc<Mutex<i64>>,
    ) -> FsResult<()> {
        let mut complete = false;
        let path_type = Self::path_type(writer.path());
        while let Some(task) = req_receiver.recv().await {
            match task {
                WriteTask::Write(off, data, reply) => {
                    FuseMetrics::get()
                        .stream_queue_depth
                        .with_label_values(&["write"])
                        .dec();
                    let len = data.len();
                    let start = Instant::now();
                    let res: FsResult<fuse_write_out> = writer
                        .fuse_write(off, DataSlice::Bytes(data))
                        .await
                        .map(|_| fuse_write_out {
                            size: len as u32,
                            padding: 0,
                        });
                    let status = status_label(res.is_ok());
                    FuseMetrics::get().observe_io(
                        "write",
                        path_type,
                        status,
                        if res.is_ok() { len } else { 0 },
                        elapsed_us(start),
                        Some(len),
                    );
                    if res.is_err() {
                        FuseMetrics::get().observe_io_error("write", path_type, "unknown");
                    }

                    if res.is_ok() {
                        let mut lock = file_len.lock().unwrap();
                        *lock = lock.max(off + len as i64);
                    }

                    reply.send_rep(res).await?;
                }

                WriteTask::Flush(tx, reply) => {
                    FuseMetrics::get()
                        .stream_queue_depth
                        .with_label_values(&["flush"])
                        .dec();
                    let start = Instant::now();
                    let res = writer.flush().await;
                    let status = status_label(res.is_ok());
                    FuseMetrics::get().observe_io(
                        "flush",
                        path_type,
                        status,
                        0,
                        elapsed_us(start),
                        None,
                    );
                    if res.is_err() {
                        FuseMetrics::get().observe_io_error("flush", path_type, "unknown");
                    }
                    if let Some(reply) = reply {
                        reply.send_rep(res).await?;
                    }
                    tx.send(1)?;
                }

                WriteTask::Complete(tx, reply) => {
                    FuseMetrics::get()
                        .stream_queue_depth
                        .with_label_values(&["release"])
                        .dec();
                    let start = Instant::now();
                    let res = if !complete {
                        let res = writer.complete().await;
                        if res.is_ok() {
                            complete = true;
                        }
                        res
                    } else {
                        Ok(())
                    };
                    let status = status_label(res.is_ok());
                    FuseMetrics::get().observe_io(
                        "release",
                        path_type,
                        status,
                        0,
                        elapsed_us(start),
                        None,
                    );
                    if res.is_err() {
                        FuseMetrics::get().observe_io_error("release", path_type, "unknown");
                    }

                    if let Some(reply) = reply {
                        reply.send_rep(res).await?;
                    }
                    tx.send(1)?;
                }

                WriteTask::Resize(tx, opts) => {
                    writer.resize(opts).await?;
                    tx.send(1)?;
                }
            }
        }

        Ok(())
    }
}
