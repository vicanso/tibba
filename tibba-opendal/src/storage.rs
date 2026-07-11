// Copyright 2026 Tree xie.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::{Error, OpenDalSnafu};
use opendal::{Buffer, Metadata, OperatorInfo, Writer};
use snafu::ResultExt;
use std::time::Duration;

type Result<T, E = Error> = std::result::Result<T, E>;

/// 预签名请求结果：客户端可据此**直接**向存储后端发起请求（上传 / 下载），
/// 无需经应用服务器中转，省带宽且降时延。
///
/// 仅 S3 等具备 presign 能力的后端可用；`fs` 等本地后端调用会返回
/// OpenDAL `Unsupported` 错误。字段刻意用基础类型，方便上层直接序列化为 JSON。
#[derive(Debug, Clone)]
pub struct PresignResult {
    /// HTTP 方法：下载为 `GET`，上传为 `PUT`。
    pub method: String,
    /// 预签名 URL（已含鉴权查询参数，到期自动失效）。
    pub url: String,
    /// 客户端发起请求时须一并带上的头（如 `host` / `content-type`）。
    pub headers: Vec<(String, String)>,
}

impl From<opendal::raw::PresignedRequest> for PresignResult {
    fn from(req: opendal::raw::PresignedRequest) -> Self {
        // 头值理论上均为 ASCII；个别非法值降级为空串，不影响主流程
        let headers = req
            .header()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    v.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        Self {
            method: req.method().as_str().to_string(),
            url: req.uri().to_string(),
            headers,
        }
    }
}

/// 流式写入器：包装 OpenDAL [`Writer`]，让调用方分块写入大文件而无需先将整个内容读入内存。
///
/// 错误统一转换为本 crate 的 [`Error`]，对上层屏蔽 OpenDAL 细节。典型用法：
/// `writer.write(chunk).await?` 循环写入，正常结束调用 [`StorageWriter::close`] 提交，
/// 中途失败调用 [`StorageWriter::abort`] 尽量清理半截对象。
pub struct StorageWriter {
    inner: Writer,
}

impl StorageWriter {
    /// 追加写入一块数据。
    pub async fn write(&mut self, bs: impl Into<Buffer>) -> Result<()> {
        self.inner.write(bs).await.context(OpenDalSnafu)
    }

    /// 结束写入并提交对象，返回写入后的元数据。
    pub async fn close(mut self) -> Result<Metadata> {
        self.inner.close().await.context(OpenDalSnafu)
    }

    /// 中止写入：尽量丢弃已写入的分片（如 S3 abort multipart upload），
    /// 避免上传中途失败时残留半截对象。
    pub async fn abort(mut self) -> Result<()> {
        self.inner.abort().await.context(OpenDalSnafu)
    }
}

/// 统一存储抽象，封装 OpenDAL `Operator`，对上层屏蔽具体后端实现。
pub struct Storage {
    dal: opendal::Operator,
}

impl Storage {
    /// 以给定的 OpenDAL Operator 创建 Storage 实例。
    pub fn new(dal: opendal::Operator) -> Self {
        Self { dal }
    }

    /// 将数据写入指定路径，返回写入后的元数据。
    pub async fn write(&self, path: &str, bs: impl Into<Buffer>) -> Result<Metadata> {
        self.dal.write(path, bs).await.context(OpenDalSnafu)
    }

    /// 将数据连同自定义用户元数据一起写入指定路径，返回写入后的元数据。
    pub async fn write_with(
        &self,
        path: &str,
        bs: impl Into<Buffer>,
        user_metadata: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Metadata> {
        let mut writer = self
            .dal
            .writer_with(path)
            .user_metadata(user_metadata)
            .await
            .context(OpenDalSnafu)?;
        writer.write(bs.into()).await.context(OpenDalSnafu)?;
        writer.close().await.context(OpenDalSnafu)
    }

    /// 读取指定路径的完整内容，返回字节缓冲区。
    pub async fn read(&self, path: &str) -> Result<Buffer> {
        self.dal.read(path).await.context(OpenDalSnafu)
    }

    /// 读取对象的指定字节区间 `[offset, offset + len)`，用于 HTTP Range 下载，
    /// 避免为响应一个分片而把整个对象读入内存。
    pub async fn read_range(&self, path: &str, offset: u64, len: u64) -> Result<Buffer> {
        self.dal
            .read_with(path)
            .range(offset..offset + len)
            .await
            .context(OpenDalSnafu)
    }

    /// 打开一个流式写入器，用于大文件分块上传（恒定内存占用）。
    /// 返回的 [`StorageWriter`] 需在写完后 `close`，失败时 `abort`。
    pub async fn writer(&self, path: &str) -> Result<StorageWriter> {
        let inner = self.dal.writer(path).await.context(OpenDalSnafu)?;
        Ok(StorageWriter { inner })
    }

    /// 获取指定路径的对象元数据（大小、内容类型、修改时间等）。
    pub async fn stat(&self, path: &str) -> Result<Metadata> {
        self.dal.stat(path).await.context(OpenDalSnafu)
    }

    /// 获取当前存储后端的基本信息（后端类型、根路径等）。
    pub fn info(&self) -> OperatorInfo {
        self.dal.info()
    }

    /// 探活：调用底层 OpenDAL `check()`（通常是对根路径的 stat），
    /// 用于 readiness probe，确认存储后端可达。
    pub async fn check(&self) -> Result<()> {
        self.dal.check().await.context(OpenDalSnafu)
    }

    /// 生成「下载」预签名请求：客户端可凭返回 URL 在 `expire` 内直接 `GET` 对象，
    /// 不经应用中转。仅 S3 等支持 presign 的后端可用，否则返回 `Unsupported` 错误。
    pub async fn presign_read(&self, path: &str, expire: Duration) -> Result<PresignResult> {
        let req = self
            .dal
            .presign_read(path, expire)
            .await
            .context(OpenDalSnafu)?;
        Ok(req.into())
    }

    /// 生成「上传」预签名请求：客户端可凭返回 URL 在 `expire` 内直接 `PUT` 对象。
    /// 仅 S3 等支持 presign 的后端可用，否则返回 `Unsupported` 错误。
    pub async fn presign_write(&self, path: &str, expire: Duration) -> Result<PresignResult> {
        let req = self
            .dal
            .presign_write(path, expire)
            .await
            .context(OpenDalSnafu)?;
        Ok(req.into())
    }
}
