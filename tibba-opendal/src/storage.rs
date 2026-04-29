// Copyright 2025 Tree xie.
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
use opendal::{Buffer, Metadata, OperatorInfo};
use snafu::ResultExt;

type Result<T, E = Error> = std::result::Result<T, E>;

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

    /// 获取指定路径的对象元数据（大小、内容类型、修改时间等）。
    pub async fn stat(&self, path: &str) -> Result<Metadata> {
        self.dal.stat(path).await.context(OpenDalSnafu)
    }

    /// 获取当前存储后端的基本信息（后端类型、根路径等）。
    pub fn info(&self) -> OperatorInfo {
        self.dal.info()
    }
}
