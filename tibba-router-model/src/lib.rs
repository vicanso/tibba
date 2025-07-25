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

use once_cell::sync::Lazy;
use strum::EnumString;
use tibba_model::{
    ConfigurationModel, DetectorGroupModel, FileModel, HttpDetectorModel, HttpStatModel, Model,
    UserModel, WebPageDetectorModel,
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
}

pub static USER_MODEL: Lazy<UserModel> = Lazy::new(UserModel::new);
pub static CONFIGURATION_MODEL: Lazy<ConfigurationModel> = Lazy::new(ConfigurationModel::new);
pub static FILE_MODEL: Lazy<FileModel> = Lazy::new(FileModel::new);
pub static HTTP_DETECTOR_MODEL: Lazy<HttpDetectorModel> = Lazy::new(HttpDetectorModel::new);
pub static HTTP_STAT_MODEL: Lazy<HttpStatModel> = Lazy::new(HttpStatModel::new);
pub static WEB_PAGE_DETECTOR_MODEL: Lazy<WebPageDetectorModel> =
    Lazy::new(WebPageDetectorModel::new);
pub static DETECTOR_GROUP_MODEL: Lazy<DetectorGroupModel> = Lazy::new(DetectorGroupModel::new);

pub use router::*;
