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

pub(crate) use tibba_model::{
    Error, JsonSnafu, ModelListParams, Schema, SchemaAllowCreate, SchemaAllowEdit, SchemaOption,
    SchemaOptionValue, SchemaType, SchemaView, SqlxSnafu, Status, format_datetime,
    new_schema_options,
};

// ── 充值来源常量 ──────────────────────────────────────────────────────────────

/// 充值来源：购买
pub const RECHARGE_SOURCE_PURCHASE: i16 = 1;
/// 充值来源：系统赠送
pub const RECHARGE_SOURCE_GIFT: i16 = 2;
/// 充值来源：退款
pub const RECHARGE_SOURCE_REFUND: i16 = 3;
/// 充值来源：管理员调整
pub const RECHARGE_SOURCE_ADMIN: i16 = 4;

// ── 服务类型常量 ──────────────────────────────────────────────────────────────

/// 服务类型：大语言模型
pub const SERVICE_LLM: &str = "llm";
/// 服务类型：通用 API 调用
pub const SERVICE_API: &str = "api";
/// 服务类型：文件存储
pub const SERVICE_STORAGE: &str = "storage";
/// 服务类型：管理员扣减额度（写入 token_usages.service，区别于真实消费）
pub const SERVICE_ADMIN_ADJUST: &str = "admin_adjust";

mod account;
mod key;
mod llm;
mod price;
mod recharge;
mod service;
mod usage;

pub use account::*;
pub use key::*;
pub use llm::*;
pub use price::*;
pub use recharge::*;
pub use service::*;
pub use usage::*;
