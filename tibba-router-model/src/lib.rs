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

use std::sync::LazyLock;
use strum::EnumString;
use tibba_model_builtin::{
    ConfigurationModel, DetectorGroupModel, DetectorGroupUserModel, FileModel, HttpDetectorModel,
    HttpStatModel, Model, UserModel, WebPageDetectorModel,
};
use tibba_model_token::{
    TokenAccountModel, TokenKeyModel, TokenPriceModel, TokenRechargeModel, TokenUsageModel,
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
    TokenAccount,
    TokenKey,
    TokenRecharge,
    TokenUsage,
    TokenPrice,
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
pub static TOKEN_KEY_MODEL: LazyLock<TokenKeyModel> = LazyLock::new(TokenKeyModel::new);
pub static TOKEN_ACCOUNT_MODEL: LazyLock<TokenAccountModel> = LazyLock::new(TokenAccountModel::new);
pub static TOKEN_RECHARGE_MODEL: LazyLock<TokenRechargeModel> =
    LazyLock::new(TokenRechargeModel::new);
pub static TOKEN_USAGE_MODEL: LazyLock<TokenUsageModel> = LazyLock::new(TokenUsageModel::new);
pub static TOKEN_PRICE_MODEL: LazyLock<TokenPriceModel> = LazyLock::new(TokenPriceModel::new);

pub use router::*;
