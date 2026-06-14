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

//! 应用级异步任务 handler 注册。
//!
//! 这里把「注册赠积分」从原先的 fire-and-forget（`let _ = recharge().await`，失败即丢）
//! 改为入队的 `gift_points` 任务：注册流程只负责入队，真正充值由 worker 执行，
//! **失败会自动重试**（多次失败转死信），不再静默丢分。

use crate::sql::get_db_pool;
use std::sync::Arc;
use tibba_error::Error as BaseError;
use tibba_job::{BoxFuture, JobContext, JobHandler, register_handler};
use tibba_model_token::{RECHARGE_SOURCE_GIFT, TokenRechargeInsertParams, TokenService};

/// 任务类型名：注册赠积分。入队与 handler 须用同一常量，避免拼写漂移。
pub const JOB_GIFT_POINTS: &str = "gift_points";

/// 注册赠送的积分数（与原 on_register 内联逻辑一致）。
const GIFT_AMOUNT: i64 = 1_000_000;

/// `gift_points` 任务处理器：给指定用户充值赠送积分。
///
/// 幂等性说明：当前按「至少一次」语义实现；若需严格一次，可在 payload 带幂等键并在
/// 充值前查重。注册赠分多发的风险可接受，MVP 暂不加额外去重。
struct GiftPointsHandler;

impl JobHandler for GiftPointsHandler {
    fn job_type(&self) -> &'static str {
        JOB_GIFT_POINTS
    }

    fn handle(&self, ctx: JobContext) -> BoxFuture<'_, std::result::Result<(), BaseError>> {
        Box::pin(async move {
            let user_id = ctx
                .payload
                .get("user_id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| {
                    BaseError::new("gift_points: payload missing user_id")
                        .with_category("job")
                        .with_sub_category("bad_payload")
                })?;

            TokenService::recharge(
                get_db_pool(),
                TokenRechargeInsertParams {
                    user_id,
                    amount: GIFT_AMOUNT,
                    source: RECHARGE_SOURCE_GIFT,
                    remark: Some("注册赠送".to_string()),
                    ..Default::default()
                },
            )
            .await?;
            Ok(())
        })
    }
}

/// 注册所有应用级任务 handler。须在 `tibba_job::start` 之前调用。
pub fn register_job_handlers() {
    register_handler(Arc::new(GiftPointsHandler));
}
