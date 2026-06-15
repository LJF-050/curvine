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

use num_enum::{FromPrimitive, IntoPrimitive};

#[repr(u32)]
#[derive(Debug, FromPrimitive, IntoPrimitive, Copy, Clone, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types)]
pub enum FuseOpCode {
    #[num_enum(default)]
    NOT_SUPPORTED = 0,

    FUSE_LOOKUP = 1,
    FUSE_FORGET = 2, // no reply
    FUSE_GETATTR = 3,
    FUSE_SETATTR = 4,
    FUSE_READLINK = 5,
    FUSE_SYMLINK = 6,
    FUSE_MKNOD = 8,
    FUSE_MKDIR = 9,
    FUSE_UNLINK = 10,
    FUSE_RMDIR = 11,
    FUSE_RENAME = 12,
    FUSE_LINK = 13,
    FUSE_OPEN = 14,
    FUSE_READ = 15,
    FUSE_WRITE = 16,
    FUSE_STATFS = 17,
    FUSE_RELEASE = 18,
    FUSE_FSYNC = 20,
    FUSE_SETXATTR = 21,
    FUSE_GETXATTR = 22,
    FUSE_LISTXATTR = 23,
    FUSE_REMOVEXATTR = 24,
    FUSE_FLUSH = 25,
    FUSE_INIT = 26,
    FUSE_OPENDIR = 27,
    FUSE_READDIR = 28,
    FUSE_RELEASEDIR = 29,
    FUSE_FSYNCDIR = 30,
    FUSE_GETLK = 31,
    FUSE_SETLK = 32,
    FUSE_SETLKW = 33,
    FUSE_ACCESS = 34,
    FUSE_CREATE = 35,
    FUSE_INTERRUPT = 36,
    FUSE_BMAP = 37,
    FUSE_DESTROY = 38,
    FUSE_IOCTL = 39,
    FUSE_POLL = 40,
    FUSE_NOTIFY_REPLY = 41,
    FUSE_BATCH_FORGET = 42,
    FUSE_FALLOCATE = 43,
    FUSE_READDIRPLUS = 44,
    FUSE_RENAME2 = 45,
    FUSE_LSEEK = 46,

    CUSE_INIT = 4096,
}

impl FuseOpCode {
    pub fn as_label(self) -> &'static str {
        match self {
            FuseOpCode::NOT_SUPPORTED => "NotSupported",
            FuseOpCode::FUSE_LOOKUP => "Lookup",
            FuseOpCode::FUSE_FORGET => "Forget",
            FuseOpCode::FUSE_GETATTR => "GetAttr",
            FuseOpCode::FUSE_SETATTR => "SetAttr",
            FuseOpCode::FUSE_READLINK => "Readlink",
            FuseOpCode::FUSE_SYMLINK => "Symlink",
            FuseOpCode::FUSE_MKNOD => "MkNod",
            FuseOpCode::FUSE_MKDIR => "Mkdir",
            FuseOpCode::FUSE_UNLINK => "Unlink",
            FuseOpCode::FUSE_RMDIR => "RmDir",
            FuseOpCode::FUSE_RENAME => "Rename",
            FuseOpCode::FUSE_LINK => "Link",
            FuseOpCode::FUSE_OPEN => "Open",
            FuseOpCode::FUSE_READ => "Read",
            FuseOpCode::FUSE_WRITE => "Write",
            FuseOpCode::FUSE_STATFS => "StatFs",
            FuseOpCode::FUSE_RELEASE => "Release",
            FuseOpCode::FUSE_FSYNC => "FSync",
            FuseOpCode::FUSE_SETXATTR => "SetXAttr",
            FuseOpCode::FUSE_GETXATTR => "GetXAttr",
            FuseOpCode::FUSE_LISTXATTR => "ListXAttr",
            FuseOpCode::FUSE_REMOVEXATTR => "RemoveXAttr",
            FuseOpCode::FUSE_FLUSH => "Flush",
            FuseOpCode::FUSE_INIT => "Init",
            FuseOpCode::FUSE_OPENDIR => "OpenDir",
            FuseOpCode::FUSE_READDIR => "ReadDir",
            FuseOpCode::FUSE_RELEASEDIR => "ReleaseDir",
            FuseOpCode::FUSE_FSYNCDIR => "FSyncDir",
            FuseOpCode::FUSE_GETLK => "GetLk",
            FuseOpCode::FUSE_SETLK => "SetLk",
            FuseOpCode::FUSE_SETLKW => "SetLkW",
            FuseOpCode::FUSE_ACCESS => "Access",
            FuseOpCode::FUSE_CREATE => "Create",
            FuseOpCode::FUSE_INTERRUPT => "Interrupt",
            FuseOpCode::FUSE_BMAP => "Bmap",
            FuseOpCode::FUSE_DESTROY => "Destroy",
            FuseOpCode::FUSE_IOCTL => "Ioctl",
            FuseOpCode::FUSE_POLL => "Poll",
            FuseOpCode::FUSE_NOTIFY_REPLY => "NotifyReply",
            FuseOpCode::FUSE_BATCH_FORGET => "BatchForget",
            FuseOpCode::FUSE_FALLOCATE => "FAllocate",
            FuseOpCode::FUSE_READDIRPLUS => "ReadDirPlus",
            FuseOpCode::FUSE_RENAME2 => "Rename2",
            FuseOpCode::FUSE_LSEEK => "Lseek",
            FuseOpCode::CUSE_INIT => "CuseInit",
        }
    }

    pub fn kind_label(self) -> &'static str {
        match self {
            FuseOpCode::FUSE_READ
            | FuseOpCode::FUSE_WRITE
            | FuseOpCode::FUSE_FLUSH
            | FuseOpCode::FUSE_RELEASE
            | FuseOpCode::FUSE_FSYNC => "stream",
            _ => "metadata",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FuseOpCode;

    #[test]
    fn opcode_labels_are_static_and_bounded() {
        assert_eq!(FuseOpCode::FUSE_READ.as_label(), "Read");
        assert_eq!(FuseOpCode::FUSE_LOOKUP.as_label(), "Lookup");
        assert_eq!(FuseOpCode::NOT_SUPPORTED.as_label(), "NotSupported");
    }

    #[test]
    fn opcode_kind_matches_stream_ops() {
        assert_eq!(FuseOpCode::FUSE_READ.kind_label(), "stream");
        assert_eq!(FuseOpCode::FUSE_WRITE.kind_label(), "stream");
        assert_eq!(FuseOpCode::FUSE_LOOKUP.kind_label(), "metadata");
    }
}
