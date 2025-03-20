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

use snafu::Snafu;
use sqlx::MySqlPool;
use sqlx::pool::PoolOptions;
use tibba_config::DatabaseConfig;
use tibba_error::Error as BaseError;
use tibba_error::new_error;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("{source}"))]
    Sqlx { source: sqlx::Error },
}

impl From<Error> for BaseError {
    fn from(source: Error) -> Self {
        match source {
            Error::Sqlx { source } => {
                let he = new_error(&source.to_string())
                    .with_category("sqlx")
                    .with_exception(true);
                he.into()
            }
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

pub async fn new_mysql_pool(config: &DatabaseConfig) -> Result<MySqlPool> {
    let pool = PoolOptions::new()
        .connect(config.url.as_str())
        .await
        .map_err(|e| Error::Sqlx { source: e })?;
    Ok(pool)
}
