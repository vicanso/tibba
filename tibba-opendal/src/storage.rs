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

pub struct Storage {
    dal: opendal::Operator,
}

impl Storage {
    /// Create a new storage.
    pub fn new(dal: opendal::Operator) -> Self {
        Self { dal }
    }
    /// Write data to the storage.
    pub async fn write(&self, path: &str, bs: impl Into<Buffer>) -> Result<Metadata> {
        self.dal.write(path, bs).await.context(OpenDalSnafu)
    }
    /// Write data to the storage with user metadata.
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
    /// Read data from the storage.
    pub async fn read(&self, path: &str) -> Result<Buffer> {
        self.dal.read(path).await.context(OpenDalSnafu)
    }
    /// Get metadata of the storage.
    pub async fn stat(&self, path: &str) -> Result<Metadata> {
        self.dal.stat(path).await.context(OpenDalSnafu)
    }
    /// Get info of the storage.
    pub fn info(&self) -> OperatorInfo {
        self.dal.info()
    }
}
