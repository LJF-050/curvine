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

use crate::file::{FsContext, FsWriterBase, FsWriterBuffer};
use bytes::BytesMut;
use curvine_common::fs::{Path, Writer};
use curvine_common::state::{FileAllocOpts, FileBlocks, FileStatus};
use curvine_common::FsResult;
use log::debug;
use orpc::common::{elapsed_us, status_label, ByteUnit, TimeSpent};
use orpc::sys::DataSlice;
use orpc::{err_box, ternary};
use std::sync::Arc;
use std::time::Instant;

type Inner = FsWriterBuffer;

pub struct FsWriter {
    inner: Inner,
    buf: BytesMut,
    chunk_size: usize,
    pos: i64,
    append: bool,
}

impl FsWriter {
    pub fn new(fs_context: Arc<FsContext>, path: Path, status: FileBlocks, append: bool) -> Self {
        let chunk_size = fs_context.write_chunk_size();
        let chunk_num = fs_context.write_chunk_num();
        let pos = ternary!(append, status.len, 0);

        debug!(
            "Create writer, path={}, pos={}, len = {}, block_size={}, chunk_size={}, chunk_number={}, replicas={}",
            &status.path,
            pos,
            status.len,
            ByteUnit::byte_to_string(status.block_size as u64),
            chunk_size,
            chunk_num,
            status.replicas
        );

        let writer = FsWriterBase::new(fs_context, path, status, pos);
        let inner = FsWriterBuffer::new(writer, chunk_num);

        Self {
            inner,
            buf: BytesMut::with_capacity(chunk_size),
            chunk_size,
            pos,
            append,
        }
    }

    pub fn create(fs_context: Arc<FsContext>, path: Path, file_blocks: FileBlocks) -> Self {
        Self::new(fs_context, path, file_blocks, false)
    }

    pub fn append(fs_context: Arc<FsContext>, path: Path, file_blocks: FileBlocks) -> Self {
        Self::new(fs_context, path, file_blocks, true)
    }

    pub fn file_blocks(&self) -> &FileBlocks {
        self.inner.file_blocks()
    }

    fn path_type(path: &Path) -> &'static str {
        if path.is_cv() {
            "curvine"
        } else {
            "ufs"
        }
    }
}

impl Writer for FsWriter {
    fn status(&self) -> &FileStatus {
        self.inner.status()
    }

    fn path(&self) -> &Path {
        self.inner.path()
    }

    fn pos(&self) -> i64 {
        self.pos
    }

    fn pos_mut(&mut self) -> &mut i64 {
        &mut self.pos
    }

    fn chunk_mut(&mut self) -> &mut BytesMut {
        &mut self.buf
    }

    fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    async fn write_chunk(&mut self, chunk: DataSlice) -> FsResult<i64> {
        let len = chunk.len();
        let _timer =
            TimeSpent::timer_counter(Arc::new(FsContext::get_metrics().write_time_us.clone()));
        let start = Instant::now();
        let path_type = Self::path_type(self.path());
        let res = self.inner.write(chunk).await;
        let status = status_label(res.is_ok());
        FsContext::get_metrics().observe_io(
            "write",
            path_type,
            status,
            if res.is_ok() { len } else { 0 },
            elapsed_us(start),
            Some(len),
        );
        if let Err(e) = &res {
            FsContext::get_metrics().observe_io_error("write", path_type, e);
        }
        res?;
        FsContext::get_metrics().write_bytes.inc_by(len as i64);
        Ok(len as i64)
    }

    async fn flush(&mut self) -> FsResult<()> {
        self.flush_chunk().await?;
        let start = Instant::now();
        let path_type = Self::path_type(self.path());
        let res = self.inner.flush().await;
        let status = status_label(res.is_ok());
        FsContext::get_metrics().observe_io("flush", path_type, status, 0, elapsed_us(start), None);
        if let Err(e) = &res {
            FsContext::get_metrics().observe_io_error("flush", path_type, e);
        }
        res
    }

    // Write is completed, perform the following operations
    // 1. Submit the last block.
    async fn complete(&mut self) -> FsResult<()> {
        self.flush_chunk().await?;
        // The flush operation will be automatically called internally, so flush is not needed here.
        let start = Instant::now();
        let path_type = Self::path_type(self.path());
        let res = self.inner.complete().await;
        let status = status_label(res.is_ok());
        FsContext::get_metrics().observe_io(
            "complete",
            path_type,
            status,
            0,
            elapsed_us(start),
            None,
        );
        if let Err(e) = &res {
            FsContext::get_metrics().observe_io_error("complete", path_type, e);
        }
        res
    }

    async fn cancel(&mut self) -> FsResult<()> {
        Ok(())
    }

    async fn seek(&mut self, pos: i64) -> FsResult<()> {
        if pos < 0 {
            return err_box!(format!("Cannot seek to negative position: {}", pos));
        }

        if self.append {
            debug!(
                "Seek operation in append mode is ineffective,\
             data will still be written in append mode"
            );
            return Ok(());
        }

        // Flush current buffer
        self.flush_chunk().await?;

        // Delegate to inner writer to execute seek
        self.inner.seek(pos).await?;

        // Update current position
        self.pos = pos;
        Ok(())
    }

    async fn resize(&mut self, opts: FileAllocOpts) -> FsResult<()> {
        self.flush_chunk().await?;
        self.inner.resize(opts).await
    }
}

impl Drop for FsWriter {
    fn drop(&mut self) {
        debug!("Close writer, path={}", self.path())
    }
}
