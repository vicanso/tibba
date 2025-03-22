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

use opendal::Operator;
use opendal::layers::MimeGuessLayer;
use snafu::Snafu;
use tibba_config::OpenDalConfig;
use tibba_error::{Error as BaseError, new_error};
#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("open dal {source}"))]
    OpenDal { source: opendal::Error },
}

impl From<Error> for BaseError {
    fn from(source: Error) -> Self {
        let error_category = "open_dal";
        match source {
            Error::OpenDal { source } => {
                let he = new_error(&source.to_string())
                    .with_category(error_category)
                    .with_sub_category("open_dal")
                    .with_exception(true);
                he.into()
            }
        }
    }
}

type Result<T, E = Error> = std::result::Result<T, E>;

pub struct Storage {
    pub dal: opendal::Operator,
}

pub fn new_open_dal_storage(config: &OpenDalConfig) -> Result<Storage> {
    let builder = opendal::services::Mysql::default().connection_string(config.url.as_str());

    let dal = Operator::new(builder)
        .map_err(|e| Error::OpenDal { source: e })?
        .layer(MimeGuessLayer::default())
        .finish();
    Ok(Storage { dal })
}
