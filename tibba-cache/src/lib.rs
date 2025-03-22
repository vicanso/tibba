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
use tibba_error::new_error;

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
        let error_category = "cache";
        match val {
            Error::Common { category, message } => new_error(&message)
                .with_category(error_category)
                .with_sub_category(&category),
            Error::SingleBuild { source } => new_error(&source.to_string())
                .with_category(error_category)
                .with_sub_category("single_build")
                .with_status(500)
                .with_exception(true),
            Error::ClusterBuild { source } => new_error(&source.to_string())
                .with_category(error_category)
                .with_sub_category("cluster_build")
                .with_status(500)
                .with_exception(true),
            Error::Redis { category, source } => new_error(&source.to_string())
                .with_category(error_category)
                .with_sub_category(&category)
                .with_status(500)
                .with_exception(true),
            Error::Compression { source } => new_error(&source.to_string())
                .with_category(error_category)
                .with_sub_category("compression")
                .with_exception(true),
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
