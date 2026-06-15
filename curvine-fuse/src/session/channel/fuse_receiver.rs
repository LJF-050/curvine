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

use crate::fs::operator::FuseOperator;
use crate::fs::FileSystem;
use crate::raw::fuse_abi::fuse_out_header;
use crate::session::{FuseRequest, FuseResponse, FuseTask};
use crate::{err_fuse, FuseMetrics, FuseResult, FUSE_IN_HEADER_LEN};
use libc::{EAGAIN, EINTR, ENODEV, ENOENT};
use log::{debug, error, info};
use orpc::common::elapsed_us;
use orpc::io::IOResult;
use orpc::runtime::{RpcRuntime, Runtime};
use orpc::sync::channel::AsyncSender;
use orpc::sync::FastDashMap;
use orpc::sys::pipe::{AsyncFd, Pipe2, PipeFd};
use orpc::{err_box, sys};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{watch, Notify};
use tokio_util::bytes::BytesMut;

/// FuseReceiver provides the following functionality:
/// 1. Receive data from fuse fd using splice
/// 2. For metadata requests (mkdir, ls), spawn a task to execute
/// 3. For file read/write requests, send task to queue
pub struct FuseReceiver<T> {
    kernel_fd: Arc<AsyncFd>,
    fs: Arc<T>,
    rt: Arc<Runtime>,
    sender: AsyncSender<FuseTask>,
    pipe2: Pipe2,
    buf: BytesMut,
    fuse_len: usize,
    debug: bool,
    audit_logging_enabled: bool,
    pending_requests: Arc<FastDashMap<u64, Arc<Notify>>>,
}

impl<T: FileSystem> FuseReceiver<T> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fs: Arc<T>,
        rt: Arc<Runtime>,
        kernel_fd: Arc<AsyncFd>,
        sender: AsyncSender<FuseTask>,
        buf_size: usize,
        debug: bool,
        audit_logging_enabled: bool,
        pending_requests: Arc<FastDashMap<u64, Arc<Notify>>>,
    ) -> IOResult<Self> {
        let pipe2 = Pipe2::new(PipeFd::new(buf_size, false, false)?)?;
        let buf = BytesMut::zeroed(buf_size);

        let client = Self {
            kernel_fd,
            fs,
            rt,
            sender,
            pipe2,
            buf,
            fuse_len: buf_size,
            debug,
            audit_logging_enabled,
            pending_requests,
        };

        Ok(client)
    }

    // Read a data from fuse.
    pub async fn receive(&mut self) -> IOResult<BytesMut> {
        self.splice().await
    }

    // Use libc::read to read data, test it, and there are multiple memory copies.
    pub async fn read(&mut self) -> IOResult<BytesMut> {
        let len = self
            .kernel_fd
            .async_read(|fd| sys::read(fd.fd(), &mut self.buf))
            .await
            .unwrap();
        Ok(BytesMut::from(&self.buf[..len as usize]))
    }

    pub async fn splice(&mut self) -> IOResult<BytesMut> {
        let write_len = self
            .pipe2
            .write_io(&self.kernel_fd, None, self.fuse_len)
            .await
            .unwrap();

        self.buf.reserve(write_len);
        unsafe {
            self.buf.set_len(write_len);
        }

        let read_len = self.pipe2.read_buf(&mut self.buf[..write_len]).await?;
        if write_len != read_len {
            return err_box!(
                "splice read and write lengths are inconsistent, write len {}, read len {}",
                write_len,
                read_len
            );
        }
        if read_len < FUSE_IN_HEADER_LEN {
            return err_box!("short read on fuse device");
        };

        let req_buf = self.buf.split_to(read_len);
        Ok(req_buf)
    }

    pub fn new_replay(&self, req: &FuseRequest, request_start: Instant) -> FuseResponse {
        FuseResponse::new(
            req.unique(),
            req.opcode(),
            request_start,
            self.sender.clone(),
            self.debug,
        )
    }

    fn audit(&self, req: &FuseRequest) {
        if !self.audit_logging_enabled {
            return;
        }
        let ino = req.get_header().map(|h| h.nodeid).unwrap_or(0);
        info!(
            target: "audit",
            "unique={} ino={} opcode={:?}",
            req.unique(),
            ino,
            req.opcode(),
        );
    }

    pub async fn send_stream(&self, req: FuseRequest, request_start: Instant) -> FuseResult<()> {
        let operation_start = Instant::now();
        let operator = match req.parse_operator() {
            Ok(v) => v,
            Err(e) => {
                self.new_replay(&req, request_start)
                    .with_operation_start(operation_start)
                    .send_rep::<(), _>(Err(e))
                    .await?;
                return Ok(());
            }
        };

        let rep = self
            .new_replay(&req, request_start)
            .with_operation_start(operation_start);
        let error_reply = rep.clone();
        let res = match operator {
            FuseOperator::Read(op) => self.fs.read(op, rep).await,

            FuseOperator::Write(op) => self.fs.write(op, rep).await,

            FuseOperator::Flush(op) => self.fs.flush(op, rep).await,

            FuseOperator::Release(op) => self.fs.release(op, rep).await,

            FuseOperator::FSync(op) => self.fs.fsync(op, rep).await,

            _ => {
                FuseMetrics::get()
                    .unsupported_total
                    .with_label_values(&[req.opcode().as_label()])
                    .inc();
                err_fuse!(libc::ENOSYS, "unsupported operation {:?}", req.opcode())
            }
        };

        if res.is_err() {
            error_reply.send_rep(res).await?;
        }
        Ok(())
    }

    pub async fn start(mut self, mut shutdown_rx: watch::Receiver<bool>) -> FuseResult<()> {
        debug!("fuse receiver started");
        FuseMetrics::get()
            .receiver_tasks
            .with_label_values(&["running"])
            .inc();
        loop {
            let receive_start = Instant::now();
            tokio::select! {
                res = self.receive() => {
                    match res {
                        Ok(buf) => {
                            let receive_us = elapsed_us(receive_start);
                            let parse_start = Instant::now();
                            let req = match FuseRequest::from_bytes(buf.freeze()) {
                                Ok(v) => v,
                                Err(e) => {
                                    FuseMetrics::get().observe_stage(
                                        "parse",
                                        "framework",
                                        "error",
                                        elapsed_us(parse_start),
                                    );
                                    return Err(e.into());
                                }
                            };
                            let kind = req.opcode().kind_label();
                            FuseMetrics::get().observe_stage("receive", kind, "success", receive_us);
                            FuseMetrics::get().observe_stage(
                                "parse",
                                kind,
                                "success",
                                elapsed_us(parse_start),
                            );
                            FuseMetrics::get()
                                .inflight_requests
                                .with_label_values(&[kind])
                                .inc();
                            let request_start = Instant::now();

                            if self.debug {
                                match req.parse_operator() {
                                    Ok(operator) => info!(
                                        "receive unique: {}, code: {:?}, op: {:?}",
                                        req.unique(),
                                        req.opcode(),
                                        operator
                                    ),
                                    Err(e) => error!("failed to parse debug operator: {}", e),
                                }
                            }

                            if req.is_stream() {
                                if let Err(e) = self.send_stream(req, request_start).await {
                                    error!("failed to dispatch stream request: {}", e);
                                }
                            } else {
                                self.audit(&req);

                                let reply = self.new_replay(&req, request_start);
                                let fs = self.fs.clone();
                                let pending_requests = self.pending_requests.clone();
                                self.rt.spawn(async move {
                                    if let Err(e) = Self::dispatch_meta_interrupt(fs, pending_requests, req, reply).await {
                                        error!("failed to dispatch meta request: {}", e);
                                    }
                                });
                            }
                        }

                        Err(e) => match e.raw_error().raw_os_error() {
                            Some(ENOENT) => continue,
                            Some(EINTR) => continue,
                            Some(EAGAIN) => continue,
                            Some(ENODEV) => break,
                            _ => {
                                FuseMetrics::get().observe_stage(
                                    "receive",
                                    "framework",
                                    "error",
                                    elapsed_us(receive_start),
                                );
                                return Err(e.into());
                            },
                        },
                    }
                }

                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("receiver observed shutdown broadcast; exiting receive loop");
                        break;
                    }
                }
            }
        }

        FuseMetrics::get()
            .receiver_tasks
            .with_label_values(&["running"])
            .dec();
        Ok(())
    }

    pub async fn dispatch_meta_interrupt(
        fs: Arc<T>,
        pending_requests: Arc<FastDashMap<u64, Arc<Notify>>>,
        req: FuseRequest,
        reply: FuseResponse,
    ) -> FuseResult<()> {
        if !req.is_interrupt() {
            return Self::dispatch_meta(&pending_requests, &fs, &req, &reply).await;
        }

        let notify = Arc::new(Notify::new());
        pending_requests.insert(req.unique(), notify.clone());
        FuseMetrics::get().pending_interruptible_requests.inc();

        let res = tokio::select! {
            result = Self::dispatch_meta(&pending_requests, &fs, &req, &reply) => {
                pending_requests.remove(&req.unique());
                FuseMetrics::get().pending_interruptible_requests.dec();
                result
            }

            _ = notify.notified() => {
                pending_requests.remove(&req.unique());
                FuseMetrics::get().pending_interruptible_requests.dec();
                FuseMetrics::get()
                    .interrupted_total
                    .with_label_values(&[req.opcode().as_label()])
                    .inc();
                let err: FuseResult<()> = err_fuse!(EINTR, "operation interrupted");
                reply.send_rep(err).await.map_err(|x| x.into())
            }
        };

        res
    }

    pub async fn dispatch_meta(
        pending_requests: &FastDashMap<u64, Arc<Notify>>,
        fs: &T,
        req: &FuseRequest,
        reply: &FuseResponse,
    ) -> FuseResult<()> {
        let reply = reply.clone().with_operation_start(Instant::now());
        let operator = match req.parse_operator() {
            Ok(v) => v,
            Err(e) => {
                reply.send_rep::<(), _>(Err(e)).await?;
                return Ok(());
            }
        };

        let res = match operator {
            FuseOperator::Init(op) => reply.send_rep(fs.init(op).await).await,

            FuseOperator::StatFs(op) => reply.send_rep(fs.stat_fs(op).await).await,

            FuseOperator::Access(op) => reply.send_rep(fs.access(op).await).await,

            FuseOperator::Lookup(op) => reply.send_rep(fs.lookup(op).await).await,

            FuseOperator::GetAttr(op) => reply.send_rep(fs.get_attr(op).await).await,

            FuseOperator::SetAttr(op) => reply.send_rep(fs.set_attr(op).await).await,

            FuseOperator::GetXAttr(op) => reply.send_buf(fs.get_xattr(op).await).await,

            FuseOperator::SetXAttr(op) => reply.send_rep(fs.set_xattr(op).await).await,

            FuseOperator::RemoveXAttr(op) => reply.send_rep(fs.remove_xattr(op).await).await,

            FuseOperator::ListXAttr(op) => reply.send_buf(fs.list_xattr(op).await).await,

            FuseOperator::OpenDir(op) => reply.send_rep(fs.open_dir(op).await).await,

            FuseOperator::Mkdir(op) => reply.send_rep(fs.mkdir(op).await).await,

            FuseOperator::FAllocate(op) => reply.send_rep(fs.allocate(op).await).await,

            FuseOperator::ReleaseDir(op) => reply.send_rep(fs.release_dir(op).await).await,

            FuseOperator::ReadDir(op) => {
                let res = fs.read_dir(op).await.map(|x| x.take());
                reply.send_buf(res).await
            }

            FuseOperator::ReadDirPlus(op) => {
                let res = fs.read_dir_plus(op).await.map(|x| x.take());
                reply.send_buf(res).await
            }

            FuseOperator::Forget(op) => reply.send_none(fs.forget(op).await),

            FuseOperator::Open(op) => reply.send_rep(fs.open(op).await).await,

            FuseOperator::MkNod(op) => reply.send_rep(fs.mk_nod(op).await).await,

            FuseOperator::Create(op) => reply.send_rep(fs.create(op).await).await,

            FuseOperator::Unlink(op) => reply.send_rep(fs.unlink(op).await).await,

            FuseOperator::RmDir(op) => reply.send_rep(fs.rm_dir(op).await).await,

            FuseOperator::Link(op) => reply.send_rep(fs.link(op).await).await,

            FuseOperator::BatchForget(op) => reply.send_none(fs.batch_forget(op).await),

            FuseOperator::Rename(op) => reply.send_rep(fs.rename(op).await).await,

            FuseOperator::Interrupt(op) => {
                let res = if let Some(notify) = pending_requests.get(&op.arg.unique) {
                    notify.notify_one();
                    Ok(())
                } else {
                    fs.interrupt(op).await
                };
                reply.send_rep(res).await
            }

            FuseOperator::Symlink(op) => reply.send_rep(fs.symlink(op).await).await,

            FuseOperator::Readlink(op) => reply.send_buf(fs.readlink(op).await).await,

            FuseOperator::GetLk(op) => reply.send_rep(fs.get_lk(op).await).await,

            FuseOperator::SetLk(op) => reply.send_rep(fs.set_lk(op).await).await,

            FuseOperator::SetLkW(op) => reply.send_rep(fs.set_lkw(op).await).await,

            _ => {
                FuseMetrics::get()
                    .unsupported_total
                    .with_label_values(&[req.opcode().as_label()])
                    .inc();
                let err: FuseResult<fuse_out_header> =
                    err_fuse!(libc::ENOSYS, "unsupported operation {:?}", req.opcode());
                reply.send_rep(err).await
            }
        };

        res?;
        Ok(())
    }
}
