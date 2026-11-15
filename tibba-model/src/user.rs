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

use super::{
    CryptoSnafu, Error, JsonSnafu, Model, ModelListParams, Schema, SchemaAllowEdit, SchemaOption,
    SchemaOptionValue, SchemaType, SchemaView, SqlxSnafu, Status, checked_db_id, ensure_affected,
    format_datetime, new_schema_options,
};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use sqlx::FromRow;
use sqlx::types::Json;
use sqlx::{Pool, Postgres, QueryBuilder};
use std::collections::HashMap;
use std::fmt;
use time::PrimitiveDateTime;
type Result<T> = std::result::Result<T, Error>;

pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_SUPER_ADMIN: &str = "su";

#[derive(FromRow)]
struct UserSchema {
    id: i64,
    status: i16,
    created: PrimitiveDateTime,
    modified: PrimitiveDateTime,
    account: String,
    password: String,
    nickname: Option<String>,
    phone: Option<String>,
    roles: Option<Json<Vec<String>>>,
    groups: Option<Json<Vec<String>>>,
    remark: Option<String>,
    email: Option<String>,
    avatar: Option<String>,
    last_login_at: Option<PrimitiveDateTime>,
    /// 邮箱验证通过时间；NULL 表示未验证
    email_verified_at: Option<PrimitiveDateTime>,
}

#[derive(Deserialize, Serialize)]
pub struct User {
    pub id: i64,
    pub status: i16,
    pub created: String,
    pub modified: String,
    pub account: String,
    #[serde(skip_serializing, skip_deserializing, default)]
    pub password: String,
    pub nickname: Option<String>,
    pub phone: Option<String>,
    pub roles: Option<Vec<String>>,
    pub groups: Option<Vec<String>>,
    pub remark: Option<String>,
    pub email: Option<String>,
    pub avatar: Option<String>,
    pub last_login_at: Option<String>,
    /// 邮箱验证通过时间；None 表示未验证
    pub email_verified_at: Option<String>,
}

impl fmt::Debug for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("status", &self.status)
            .field("account", &self.account)
            .field("password", &"<redacted>")
            .field("nickname", &self.nickname)
            .field("roles", &self.roles)
            .field("groups", &self.groups)
            .field("created", &self.created)
            .field("modified", &self.modified)
            .finish()
    }
}

impl User {
    /// 账号是否处于启用状态。禁用账号不得建立任何形式的登录态
    /// （Session / JWT / API Key / OAuth）。
    pub fn is_enabled(&self) -> bool {
        self.status == Status::Enabled as i16
    }
}

impl From<UserSchema> for User {
    fn from(user: UserSchema) -> Self {
        Self {
            id: user.id,
            status: user.status,
            created: format_datetime(user.created),
            modified: format_datetime(user.modified),
            account: user.account,
            password: user.password,
            nickname: user.nickname,
            phone: user.phone,
            roles: user.roles.map(|roles| roles.0),
            groups: user.groups.map(|groups| groups.0),
            remark: user.remark,
            email: user.email,
            avatar: user.avatar,
            last_login_at: user.last_login_at.map(format_datetime),
            email_verified_at: user.email_verified_at.map(format_datetime),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UserUpdateParams {
    pub nickname: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub avatar: Option<String>,
    pub roles: Option<Vec<String>>,
    pub groups: Option<Vec<String>>,
    pub status: Option<i16>,
}

fn is_valid_role_or_group_item(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b':' | b'*'))
}

fn is_valid_email_str(s: &str) -> bool {
    if s.len() > 254
        || s.bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
    {
        return false;
    }
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

impl UserUpdateParams {
    /// 校验用户更新参数，防止后台或资料接口传入超长字符串、非法角色标识或畸形邮箱。
    pub fn validate_params(&self) -> Result<()> {
        if let Some(nickname) = &self.nickname
            && nickname.chars().count() > 100
        {
            return Err(Error::Validation {
                message: "nickname length must not exceed 100".to_string(),
            });
        }
        if let Some(phone) = &self.phone
            && phone.chars().count() > 64
        {
            return Err(Error::Validation {
                message: "phone length must not exceed 64".to_string(),
            });
        }
        if let Some(avatar) = &self.avatar
            && (avatar.chars().count() > 512 || avatar.chars().any(char::is_control))
        {
            return Err(Error::Validation {
                message: "invalid avatar url".to_string(),
            });
        }
        if let Some(email) = &self.email
            && !email.is_empty()
            && !is_valid_email_str(email)
        {
            return Err(Error::Validation {
                message: "invalid email address".to_string(),
            });
        }
        if let Some(status) = self.status
            && status != Status::Disabled as i16
            && status != Status::Enabled as i16
        {
            return Err(Error::Validation {
                message: format!("invalid user status: {status}"),
            });
        }
        if let Some(roles) = &self.roles
            && (roles.len() > 32 || !roles.iter().all(|r| is_valid_role_or_group_item(r)))
        {
            return Err(Error::Validation {
                message: "invalid roles list".to_string(),
            });
        }
        if let Some(groups) = &self.groups
            && (groups.len() > 32 || !groups.iter().all(|g| is_valid_role_or_group_item(g)))
        {
            return Err(Error::Validation {
                message: "invalid groups list".to_string(),
            });
        }
        Ok(())
    }
}

/// TOTP 两步验证的最小鉴权态。
///
/// 刻意与对外序列化的 [`User`] 隔离——密钥与恢复码哈希绝不进入 `User`，
/// 避免经 `/me`、admin model 视图等任何 JSON 出口泄漏。手写 `Debug` 不暴露密文与哈希内容。
#[derive(Clone, Default)]
pub struct TotpState {
    /// 加密后的密钥 base64；`None` 表示未注册 2FA。
    pub secret_cipher: Option<String>,
    /// 是否已激活（`totp_enabled_at` 非空）。
    pub enabled: bool,
    /// 一次性恢复码哈希列表（未启用或无剩余时为空）。
    pub recovery_hashes: Vec<String>,
}

impl fmt::Debug for TotpState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TotpState")
            .field("has_secret", &self.secret_cipher.is_some())
            .field("enabled", &self.enabled)
            .field("recovery_hashes_count", &self.recovery_hashes.len())
            .finish()
    }
}

/// `search_options` 轻量查询投影，避免 `SELECT *` 拉取 `password` 与资料列。
#[derive(FromRow)]
struct UserOptionRow {
    id: i64,
    account: String,
}

/// 仅用于读取 users 表 TOTP 三列的 FromRow 目标。
#[derive(FromRow)]
struct TotpRow {
    totp_secret: Option<String>,
    totp_enabled_at: Option<PrimitiveDateTime>,
    totp_recovery_codes: Option<Json<Vec<String>>>,
}

pub struct UserModel {}

impl Model for UserModel {
    type Output = User;
    fn new() -> Self {
        Self {}
    }
    /// 不含 password 等敏感列，防止 ORDER BY 侧信道探测。
    fn orderable_columns(&self) -> &'static [&'static str] {
        &[
            "id",
            "created",
            "modified",
            "account",
            "status",
            "nickname",
            "email",
            "last_login_at",
        ]
    }
    fn keyword(&self) -> String {
        "account".to_string()
    }
    async fn schema_view(&self, _pool: &Pool<Postgres>) -> SchemaView {
        SchemaView {
            schemas: vec![
                Schema::new_id(),
                Schema {
                    name: "account".to_string(),
                    category: SchemaType::String,
                    read_only: true,
                    required: true,
                    identity: true,
                    ..Default::default()
                },
                Schema::new_status(),
                Schema {
                    name: "nickname".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema {
                    name: "phone".to_string(),
                    category: SchemaType::String,
                    ..Default::default()
                },
                Schema {
                    name: "roles".to_string(),
                    category: SchemaType::Strings,
                    options: Some(new_schema_options(&[ROLE_ADMIN, ROLE_SUPER_ADMIN])),
                    ..Default::default()
                },
                Schema {
                    name: "groups".to_string(),
                    category: SchemaType::Strings,
                    options: Some(new_schema_options(&["it", "marketing"])),
                    ..Default::default()
                },
                Schema {
                    name: "last_login_at".to_string(),
                    category: SchemaType::Date,
                    read_only: true,
                    ..Default::default()
                },
                Schema::new_created(),
                Schema::new_modified(),
            ],
            allow_edit: SchemaAllowEdit {
                owner: true,
                roles: vec![ROLE_SUPER_ADMIN.to_string()],
                ..Default::default()
            },
            ..Default::default()
        }
    }
    async fn get_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<Option<Self::Output>> {
        let id = checked_db_id(id)?;
        let result = sqlx::query_as::<_, UserSchema>(
            r#"SELECT * FROM users WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(result.map(|user| user.into()))
    }
    async fn delete_by_id(&self, pool: &Pool<Postgres>, id: u64) -> Result<()> {
        let id = checked_db_id(id)?;
        // 字面量满足 sqlx 0.9 SqlSafeStr；形状与 SOFT_DELETE_SET + ACTIVE_BY_ID_WHERE 一致
        let result = sqlx::query(
            r#"UPDATE users SET deleted_at = NOW(), modified = NOW() WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(id)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        ensure_affected(&result)
    }
    async fn update_by_id(
        &self,
        pool: &Pool<Postgres>,
        id: u64,
        data: serde_json::Value,
    ) -> Result<()> {
        let id = checked_db_id(id)?;
        let params: UserUpdateParams = serde_json::from_value(data).context(JsonSnafu)?;
        params.validate_params()?;
        let result = sqlx::query(
            r#"
            UPDATE users SET
                -- 邮箱一旦变更，旧的「已验证」标志不再成立，必须清空；
                -- UPDATE 的 SET 表达式看到的都是旧行值，这里的 email 是改之前的
                email_verified_at = CASE
                    WHEN $1::varchar IS NOT NULL AND $1::varchar IS DISTINCT FROM email THEN NULL
                    ELSE email_verified_at
                END,
                email = COALESCE($1, email),
                avatar = COALESCE($2, avatar),
                roles = COALESCE($3, roles),
                groups = COALESCE($4, groups),
                status = COALESCE($5, status),
                nickname = COALESCE($6, nickname),
                phone = COALESCE($7, phone),
                modified = NOW()
            WHERE id = $8 AND deleted_at IS NULL
            "#,
        )
        .bind(params.email.as_deref())
        .bind(params.avatar.as_deref())
        .bind(params.roles.map(Json))
        .bind(params.groups.map(Json))
        .bind(params.status)
        .bind(params.nickname.as_deref())
        .bind(params.phone.as_deref())
        .bind(id)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;

        ensure_affected(&result)
    }
    fn push_filter_conditions(
        &self,
        qb: &mut QueryBuilder<Postgres>,
        filters: &HashMap<String, String>,
    ) -> Result<()> {
        if let Some(status) = filters.get("status").and_then(|s| s.parse::<i16>().ok()) {
            qb.push(" AND status = ");
            qb.push_bind(status);
        }
        if let Some(role) = filters.get("role") {
            qb.push(" AND roles @> ");
            qb.push_bind(Json(vec![role.clone()]));
            qb.push("::jsonb");
        }
        if let Some(group) = filters.get("group") {
            qb.push(" AND groups @> ");
            qb.push_bind(Json(vec![group.clone()]));
            qb.push("::jsonb");
        }
        Ok(())
    }

    async fn count(&self, pool: &Pool<Postgres>, params: &ModelListParams) -> Result<i64> {
        let mut qb = QueryBuilder::new("SELECT COUNT(*) FROM users");
        self.push_conditions(&mut qb, params)?;
        let count = qb
            .build_query_scalar::<i64>()
            .fetch_one(pool)
            .await
            .context(SqlxSnafu)?;
        Ok(count)
    }

    async fn list(
        &self,
        pool: &Pool<Postgres>,
        params: &ModelListParams,
    ) -> Result<Vec<Self::Output>> {
        let mut qb = QueryBuilder::new("SELECT * FROM users");
        self.push_conditions(&mut qb, params)?;
        params.push_pagination(&mut qb, self.orderable_columns());
        let result = qb
            .build_query_as::<UserSchema>()
            .fetch_all(pool)
            .await
            .context(SqlxSnafu)?;
        Ok(result.into_iter().map(|u| u.into()).collect())
    }
    async fn search_options(
        &self,
        pool: &Pool<Postgres>,
        keyword: Option<String>,
    ) -> Result<Vec<SchemaOption>> {
        let params = ModelListParams {
            keyword,
            limit: 20,
            page: 1,
            ..Default::default()
        };
        let mut qb = QueryBuilder::new("SELECT id, account FROM users");
        self.push_conditions(&mut qb, &params)?;
        params.push_pagination(&mut qb, &["id", "account"]);
        let rows = qb
            .build_query_as::<UserOptionRow>()
            .fetch_all(pool)
            .await
            .context(SqlxSnafu)?;
        Ok(rows
            .into_iter()
            .map(|u| SchemaOption {
                label: u.account,
                value: SchemaOptionValue::String(u.id.to_string()),
            })
            .collect())
    }
}

impl UserModel {
    pub async fn register(
        &self,
        pool: &Pool<Postgres>,
        account: &str,
        password: &str,
    ) -> Result<u64> {
        // 入库前做 Argon2id 加盐哈希；password 为客户端 sha256(明文)，再套一层 KDF
        let password_hash =
            tibba_crypto::hash_password(password.as_bytes()).context(CryptoSnafu)?;

        // Insert user and return the last insert ID
        let row: (i64,) = sqlx::query_as(
            r#"
            INSERT INTO users (
                status, account, password
            ) VALUES (
                $1, $2, $3
            ) RETURNING id
            "#,
        )
        .bind(Status::Enabled as i16)
        .bind(account)
        .bind(&password_hash)
        .fetch_one(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(row.0 as u64)
    }

    pub async fn get_by_account(
        &self,
        pool: &Pool<Postgres>,
        account: &str,
    ) -> Result<Option<User>> {
        let result = sqlx::query_as::<_, UserSchema>(
            r#"SELECT * FROM users WHERE account = $1 AND deleted_at IS NULL"#,
        )
        .bind(account)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;

        Ok(result.map(|user| user.into()))
    }

    pub async fn update_by_account(
        &self,
        pool: &Pool<Postgres>,
        account: &str,
        params: UserUpdateParams,
    ) -> Result<()> {
        params.validate_params()?;
        let result = sqlx::query(
            r#"
            UPDATE users SET
                -- 见 update_by_id：邮箱变更即清空验证标志。此前不清，于是「先验证
                -- 自己的邮箱、再把资料邮箱改成别人的」就得到一个「已验证」的他人邮箱，
                -- 而 OAuth 自动合并正是按已验证邮箱认领本地账号的
                email_verified_at = CASE
                    WHEN $1::varchar IS NOT NULL AND $1::varchar IS DISTINCT FROM email THEN NULL
                    ELSE email_verified_at
                END,
                email = COALESCE($1, email),
                avatar = COALESCE($2, avatar),
                nickname = COALESCE($3, nickname),
                phone = COALESCE($4, phone),
                modified = NOW()
            WHERE account = $5 AND deleted_at IS NULL
            "#,
        )
        .bind(params.email.as_deref())
        .bind(params.avatar.as_deref())
        .bind(params.nickname.as_deref())
        .bind(params.phone.as_deref())
        .bind(account)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        ensure_affected(&result)
    }

    /// 按**已验证**邮箱查询用户（不含已软删除）。
    ///
    /// 用于 OAuth 自动合并：第三方提供已验证邮箱时，按邮箱认领本地账号。
    ///
    /// # 为什么必须要求本地邮箱也已验证
    /// 本地邮箱可以经 `/users/profile` 随意填写。此前（`get_by_email`）不看验证
    /// 状态，于是存在经典的**账号预劫持**：攻击者注册本地账号、把资料邮箱填成
    /// `victim@gmail.com`；受害者日后首次「用 Google 登录」，Google 给出的已验证
    /// 邮箱命中攻击者的账号，受害者的 Google 身份被挂到这个**攻击者持有密码**的
    /// 账号上——此后受害者存进去的一切，攻击者都看得到。
    ///
    /// 要求 `email_verified_at IS NOT NULL` 后，只有真正证明过邮箱归属的本地账号
    /// 才会被认领；未验证的同名邮箱落到「新建账号」分支，互不干扰。
    ///
    /// 本地 `users.email` 没有 UNIQUE 约束，多条命中时取最早注册者。
    pub async fn get_by_verified_email(
        &self,
        pool: &Pool<Postgres>,
        email: &str,
    ) -> Result<Option<User>> {
        let row: Option<UserSchema> = sqlx::query_as(
            r#"SELECT * FROM users
               WHERE email = $1 AND email_verified_at IS NOT NULL AND deleted_at IS NULL
               ORDER BY id ASC
               LIMIT 1"#,
        )
        .bind(email)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row.map(User::from))
    }

    /// 登录成功后更新 last_login_at 为当前时间。
    pub async fn update_last_login_at(&self, pool: &Pool<Postgres>, account: &str) -> Result<()> {
        sqlx::query(
            r#"UPDATE users SET last_login_at = CURRENT_TIMESTAMP WHERE account = $1 AND deleted_at IS NULL"#,
        )
        .bind(account)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(())
    }

    /// 邮箱验证通过：仅当用户**当前邮箱仍是发出验证码时的那个**才写入
    /// `email_verified_at = NOW()`。返回是否实际标记成功。
    ///
    /// # 为什么要带 `email`
    /// 验证码发往邮箱 A，但确认时只凭 user_id 标记，就存在一个竞态：给自己的
    /// 邮箱 A 申请验证 → 把资料邮箱改成 B（受害者的）→ 用 A 收到的验证码确认，
    /// 结果 B 被标成「已验证」。把邮箱作为条件写进 `WHERE`，验证的就一定是
    /// 收到验证码的那个地址；邮箱在此期间被改过则返回 `false`。
    pub async fn mark_email_verified(
        &self,
        pool: &Pool<Postgres>,
        user_id: i64,
        email: &str,
    ) -> Result<bool> {
        let result = sqlx::query(
            r#"UPDATE users SET email_verified_at = NOW(), modified = NOW()
               WHERE id = $1 AND email = $2 AND deleted_at IS NULL"#,
        )
        .bind(user_id)
        .bind(email)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(result.rows_affected() > 0)
    }

    /// 按账号追加角色（幂等）：已持有该角色、账号不存在或已删除时不做任何修改。
    ///
    /// 返回是否真正写入。用于启动时按配置引导超级管理员——取代此前「id == 1 的
    /// 首个注册用户自动成为超管」：那条规则让任何人都能在新部署上抢先注册拿到 su。
    pub async fn grant_role_by_account(
        &self,
        pool: &Pool<Postgres>,
        account: &str,
        role: &str,
    ) -> Result<bool> {
        let result = sqlx::query(
            r#"UPDATE users SET roles = roles || jsonb_build_array($2::text), modified = NOW()
               WHERE account = $1 AND deleted_at IS NULL
                 AND NOT roles @> jsonb_build_array($2::text)"#,
        )
        .bind(account)
        .bind(role)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(result.rows_affected() > 0)
    }

    /// 重置密码：用 Argon2id 哈希后覆盖 password 列。调用方传入客户端已 sha256 处理的
    /// 字符串（与 register 一致），本方法负责加盐哈希，绝不明文入库。
    pub async fn update_password(
        &self,
        pool: &Pool<Postgres>,
        user_id: i64,
        password: &str,
    ) -> Result<()> {
        let password_hash =
            tibba_crypto::hash_password(password.as_bytes()).context(CryptoSnafu)?;
        let result = sqlx::query(
            r#"UPDATE users SET password = $1, modified = NOW()
               WHERE id = $2 AND deleted_at IS NULL"#,
        )
        .bind(&password_hash)
        .bind(user_id)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        ensure_affected(&result)
    }

    /// 读取用户的 TOTP 鉴权态（密钥密文 / 是否启用 / 恢复码哈希）。
    /// 用户不存在或未注册时返回默认值（未启用）。
    pub async fn get_totp_state(&self, pool: &Pool<Postgres>, user_id: i64) -> Result<TotpState> {
        let row: Option<TotpRow> = sqlx::query_as(
            r#"SELECT totp_secret, totp_enabled_at, totp_recovery_codes
               FROM users WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;

        let Some(row) = row else {
            return Ok(TotpState::default());
        };
        Ok(TotpState {
            secret_cipher: row.totp_secret,
            enabled: row.totp_enabled_at.is_some(),
            recovery_hashes: row.totp_recovery_codes.map(|j| j.0).unwrap_or_default(),
        })
    }

    /// 写入待激活密钥：存密文，并把 `enabled_at` / 恢复码清空（重新注册时覆盖旧态）。
    pub async fn set_totp_pending(
        &self,
        pool: &Pool<Postgres>,
        user_id: i64,
        secret_cipher: &str,
    ) -> Result<()> {
        let result = sqlx::query(
            r#"UPDATE users
               SET totp_secret = $1, totp_enabled_at = NULL, totp_recovery_codes = NULL, modified = NOW()
               WHERE id = $2 AND deleted_at IS NULL"#,
        )
        .bind(secret_cipher)
        .bind(user_id)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        ensure_affected(&result)
    }

    /// 激活 2FA：置 `enabled_at = NOW()` 并写入恢复码哈希。
    /// 仅当已有待激活密钥（`totp_secret IS NOT NULL`）时生效。
    pub async fn activate_totp(
        &self,
        pool: &Pool<Postgres>,
        user_id: i64,
        recovery_hashes: &[String],
    ) -> Result<()> {
        let result = sqlx::query(
            r#"UPDATE users
               SET totp_enabled_at = NOW(), totp_recovery_codes = $1, modified = NOW()
               WHERE id = $2 AND deleted_at IS NULL AND totp_secret IS NOT NULL"#,
        )
        .bind(Json(recovery_hashes.to_vec()))
        .bind(user_id)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        ensure_affected(&result)
    }

    /// 关闭 2FA：清空密钥 / 激活时间 / 恢复码三列。
    pub async fn disable_totp(&self, pool: &Pool<Postgres>, user_id: i64) -> Result<()> {
        let result = sqlx::query(
            r#"UPDATE users
               SET totp_secret = NULL, totp_enabled_at = NULL, totp_recovery_codes = NULL, modified = NOW()
               WHERE id = $1 AND deleted_at IS NULL"#,
        )
        .bind(user_id)
        .execute(pool)
        .await
        .context(SqlxSnafu)?;
        ensure_affected(&result)
    }

    /// 原子消费一个恢复码哈希：存在则从数组移除并返回 `true`，否则 `false`。
    /// 用 `jsonb_exists` 判断包含、`-` 运算符移除元素，单条 UPDATE 保证原子性，
    /// 避免「校验—移除」两步之间的并发重放。
    pub async fn consume_recovery_code(
        &self,
        pool: &Pool<Postgres>,
        user_id: i64,
        code_hash: &str,
    ) -> Result<bool> {
        let row: Option<(i64,)> = sqlx::query_as(
            r#"UPDATE users
               SET totp_recovery_codes = totp_recovery_codes - $1, modified = NOW()
               WHERE id = $2 AND deleted_at IS NULL AND jsonb_exists(totp_recovery_codes, $1)
               RETURNING id"#,
        )
        .bind(code_hash)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .context(SqlxSnafu)?;
        Ok(row.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_and_totp_state_debug_redact_secrets() {
        let user = User {
            id: 1,
            status: 1,
            created: String::new(),
            modified: String::new(),
            account: "alice".to_string(),
            password: "$argon2id$very_secret_hash".to_string(),
            nickname: None,
            phone: None,
            roles: Some(vec!["admin".to_string()]),
            groups: None,
            remark: None,
            email: None,
            avatar: None,
            last_login_at: None,
            email_verified_at: None,
        };
        let u_dbg = format!("{user:?}");
        assert!(!u_dbg.contains("very_secret_hash"), "{u_dbg}");
        assert!(u_dbg.contains("<redacted>"), "{u_dbg}");

        let totp = TotpState {
            secret_cipher: Some("cipher_secret_xyz".to_string()),
            enabled: true,
            recovery_hashes: vec!["hash1".to_string(), "hash2".to_string()],
        };
        let t_dbg = format!("{totp:?}");
        assert!(!t_dbg.contains("cipher_secret_xyz"), "{t_dbg}");
        assert!(!t_dbg.contains("hash1"), "{t_dbg}");
        assert!(t_dbg.contains("recovery_hashes_count: 2"), "{t_dbg}");
    }

    #[test]
    fn user_deserialize_ignores_injected_password() {
        let json = serde_json::json!({
            "id": 7,
            "status": 1,
            "created": "",
            "modified": "",
            "account": "bob",
            "password": "attacker_injected_hash"
        });
        let user: User = serde_json::from_value(json).expect("deserialize");
        assert!(user.password.is_empty());
    }

    #[test]
    fn user_update_params_validation() {
        let ok = UserUpdateParams {
            email: Some("a@b.com".to_string()),
            roles: Some(vec!["admin".to_string()]),
            status: Some(1),
            ..Default::default()
        };
        assert!(ok.validate_params().is_ok());

        let bad_email = UserUpdateParams {
            email: Some("not-an-email".to_string()),
            ..Default::default()
        };
        assert!(matches!(
            bad_email.validate_params(),
            Err(Error::Validation { .. })
        ));

        let bad_role = UserUpdateParams {
            roles: Some(vec!["bad role!".to_string()]),
            ..Default::default()
        };
        assert!(matches!(
            bad_role.validate_params(),
            Err(Error::Validation { .. })
        ));

        let bad_status = UserUpdateParams {
            status: Some(9),
            ..Default::default()
        };
        assert!(matches!(
            bad_status.validate_params(),
            Err(Error::Validation { .. })
        ));
    }
}
