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

use std::sync::LazyLock;
use strum::EnumString;
use tibba_model::{
    ConfigurationModel, DetectorGroupModel, DetectorGroupUserModel, FileModel, HttpDetectorModel,
    HttpStatModel, Model, UserModel, WebPageDetectorModel,
};

mod router;

#[derive(Debug, Clone, Copy, EnumString)]
#[strum(serialize_all = "snake_case")]
enum CmsModel {
    User,
    Configuration,
    File,
    HttpDetector,
    HttpStat,
    WebPageDetector,
    DetectorGroup,
    DetectorGroupUser,
}

pub static USER_MODEL: LazyLock<UserModel> = LazyLock::new(UserModel::new);
pub static CONFIGURATION_MODEL: LazyLock<ConfigurationModel> =
    LazyLock::new(ConfigurationModel::new);
pub static FILE_MODEL: LazyLock<FileModel> = LazyLock::new(FileModel::new);
pub static HTTP_DETECTOR_MODEL: LazyLock<HttpDetectorModel> = LazyLock::new(HttpDetectorModel::new);
pub static HTTP_STAT_MODEL: LazyLock<HttpStatModel> = LazyLock::new(HttpStatModel::new);
pub static WEB_PAGE_DETECTOR_MODEL: LazyLock<WebPageDetectorModel> =
    LazyLock::new(WebPageDetectorModel::new);
pub static DETECTOR_GROUP_MODEL: LazyLock<DetectorGroupModel> =
    LazyLock::new(DetectorGroupModel::new);
pub static DETECTOR_GROUP_USER_MODEL: LazyLock<DetectorGroupUserModel> =
    LazyLock::new(DetectorGroupUserModel::new);

pub use router::*;
