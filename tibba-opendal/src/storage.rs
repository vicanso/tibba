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

use super::Error;
use opendal::{Buffer, Metadata, OperatorInfo};

type Result<T, E = Error> = std::result::Result<T, E>;

pub struct Storage {
    dal: opendal::Operator,
}

impl Storage {
    pub fn new(dal: opendal::Operator) -> Self {
        Self { dal }
    }
    pub async fn write(&self, path: &str, bs: impl Into<Buffer>) -> Result<Metadata> {
        let metadata = self.dal.write(path, bs).await.map_err(|e| Error::OpenDal {
            source: Box::new(e),
        })?;
        Ok(metadata)
    }
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
            .map_err(|e| Error::OpenDal {
                source: Box::new(e),
            })?;
        writer.write(bs.into()).await.map_err(|e| Error::OpenDal {
            source: Box::new(e),
        })?;
        let metadata = writer.close().await.map_err(|e| Error::OpenDal {
            source: Box::new(e),
        })?;
        Ok(metadata)
    }
    pub async fn read(&self, path: &str) -> Result<Buffer> {
        let bs = self.dal.read(path).await.map_err(|e| Error::OpenDal {
            source: Box::new(e),
        })?;
        Ok(bs)
    }
    pub async fn stat(&self, path: &str) -> Result<Metadata> {
        let metadata = self.dal.stat(path).await.map_err(|e| Error::OpenDal {
            source: Box::new(e),
        })?;
        Ok(metadata)
    }
    pub fn info(&self) -> OperatorInfo {
        self.dal.info()
    }
}
