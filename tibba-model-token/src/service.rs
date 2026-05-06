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

use super::{Error, SqlxSnafu, TokenRechargeInsertParams, TokenUsageInsertParams};
use snafu::ResultExt;
use sqlx::{Pool, Postgres};

type Result<T> = std::result::Result<T, Error>;

pub struct RechargeResult {
    pub recharge_id: i64,
    pub new_balance: i64,
}

pub struct ConsumeResult {
    pub usage_id: i64,
    pub new_balance: i64,
}

pub struct TokenService;

impl TokenService {
    /// 充值：在同一事务中插入充值记录并更新账户余额。
    /// 若账户不存在则自动创建。
    pub async fn recharge(
        pool: &Pool<Postgres>,
        params: TokenRechargeInsertParams,
    ) -> Result<RechargeResult> {
        let mut tx = pool.begin().await.context(SqlxSnafu)?;

        // 确保账户存在
        sqlx::query(
            r#"INSERT INTO token_accounts (user_id)
               VALUES ($1)
               ON CONFLICT (user_id) WHERE deleted_at IS NULL DO NOTHING"#,
        )
        .bind(params.user_id)
        .execute(&mut *tx)
        .await
        .context(SqlxSnafu)?;

        // 插入充值记录
        let (recharge_id,): (i64,) = sqlx::query_as(
            r#"INSERT INTO token_recharges
               (user_id, amount, source, order_id, remark, created_by)
               VALUES ($1, $2, $3, $4, $5, $6)
               RETURNING id"#,
        )
        .bind(params.user_id)
        .bind(params.amount)
        .bind(params.source)
        .bind(params.order_id.unwrap_or_default())
        .bind(params.remark.unwrap_or_default())
        .bind(params.created_by.unwrap_or(0))
        .fetch_one(&mut *tx)
        .await
        .context(SqlxSnafu)?;

        // 更新账户余额与充值汇总
        let (new_balance,): (i64,) = sqlx::query_as(
            r#"UPDATE token_accounts
               SET balance         = balance + $1,
                   total_recharged = total_recharged + $1
             WHERE user_id = $2 AND deleted_at IS NULL
             RETURNING balance"#,
        )
        .bind(params.amount)
        .bind(params.user_id)
        .fetch_one(&mut *tx)
        .await
        .context(SqlxSnafu)?;

        tx.commit().await.context(SqlxSnafu)?;

        Ok(RechargeResult {
            recharge_id,
            new_balance,
        })
    }

    /// 消费：在同一事务中扣减余额并写入消费记录。
    /// 余额不足时返回 `Error::InsufficientBalance`，不写消费记录，事务回滚。
    pub async fn consume(
        pool: &Pool<Postgres>,
        params: TokenUsageInsertParams,
    ) -> Result<ConsumeResult> {
        let mut tx = pool.begin().await.context(SqlxSnafu)?;

        // 原子扣减余额（余额不足则返回 None）
        let result: Option<(i64,)> = sqlx::query_as(
            r#"UPDATE token_accounts
               SET balance        = balance - $1,
                   total_consumed = total_consumed + $1
             WHERE user_id = $2
               AND balance >= $1
               AND status = 1
               AND deleted_at IS NULL
             RETURNING balance"#,
        )
        .bind(params.amount)
        .bind(params.user_id)
        .fetch_optional(&mut *tx)
        .await
        .context(SqlxSnafu)?;

        let new_balance = match result {
            Some(row) => row.0,
            None => return Err(Error::InsufficientBalance),
        };

        // 写入消费记录
        let (usage_id,): (i64,) = sqlx::query_as(
            r#"INSERT INTO token_usages
               (user_id, service, amount, model, input_tokens, output_tokens,
                api_path, duration_ms, biz_id, remark)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
               RETURNING id"#,
        )
        .bind(params.user_id)
        .bind(&params.service)
        .bind(params.amount)
        .bind(params.model.unwrap_or_default())
        .bind(params.input_tokens.unwrap_or(0))
        .bind(params.output_tokens.unwrap_or(0))
        .bind(params.api_path.unwrap_or_default())
        .bind(params.duration_ms.unwrap_or(0))
        .bind(params.biz_id.unwrap_or_default())
        .bind(params.remark.unwrap_or_default())
        .fetch_one(&mut *tx)
        .await
        .context(SqlxSnafu)?;

        tx.commit().await.context(SqlxSnafu)?;

        Ok(ConsumeResult {
            usage_id,
            new_balance,
        })
    }
}
