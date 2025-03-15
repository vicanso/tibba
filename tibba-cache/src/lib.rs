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
use tibba_error::Error as BaseError;
use tibba_error::HttpError;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("category: {category}, error: {message}"))]
    Common { category: String, message: String },
    #[snafu(display("{source}"))]
    SingleBuild { source: deadpool_redis::BuildError },
    #[snafu(display("{source}"))]
    ClusterBuild {
        source: deadpool_redis::cluster::CreatePoolError,
    },
    #[snafu(display("category: {category}, error: {source}"))]
    Redis {
        category: String,
        source: deadpool_redis::redis::RedisError,
    },
    #[snafu(display("{source}"))]
    Compression { source: tibba_util::Error },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        match val {
            Error::Common { category, message } => HttpError {
                message,
                category,
                ..Default::default()
            },
            Error::SingleBuild { source } => HttpError {
                message: source.to_string(),
                category: "single_build".to_string(),
                ..Default::default()
            },
            Error::ClusterBuild { source } => HttpError {
                message: source.to_string(),
                category: "cluster_build".to_string(),
                ..Default::default()
            },
            Error::Redis { category, source } => HttpError {
                message: source.to_string(),
                category,
                ..Default::default()
            },
            Error::Compression { source } => HttpError {
                message: source.to_string(),
                category: "compression".to_string(),
                ..Default::default()
            },
        }
        .into()
    }
}

mod cache;
mod pool;
mod ttl_lru_store;
mod two_level_store;

pub use cache::*;
pub use pool::*;
pub use ttl_lru_store::*;
pub use two_level_store::*;
