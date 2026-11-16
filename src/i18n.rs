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

//! 应用级 i18n 目录：登记常见错误 key 的中英文译文，启动期注册到 `tibba_middleware::i18n`。

use tibba_middleware::i18n::Catalog;

/// 默认（回退）语言。`Accept-Language` 协商不到受支持语言时返回英文。
const FALLBACK: &str = "en";

/// 构建并注册全局本地化目录。重复调用安全（已注册则忽略）。
pub fn init() {
    let catalog = Catalog::new(FALLBACK)
        // 限流 / 并发超限（tibba-middleware）
        .add(
            "en",
            "too_many_requests",
            "Too many requests, please retry later.",
        )
        .add("zh", "too_many_requests", "请求过于频繁，请稍后再试。")
        .add(
            "en",
            "rate_limited",
            "Rate limit exceeded, please slow down.",
        )
        .add("zh", "rate_limited", "已触发频率限制，请稍后再试。")
        // CSRF 校验失败（tibba-middleware）
        .add("en", "csrf_cookie_missing", "Missing CSRF cookie.")
        .add("zh", "csrf_cookie_missing", "缺少 CSRF cookie。")
        .add("en", "csrf_header_missing", "Missing CSRF header.")
        .add("zh", "csrf_header_missing", "缺少 CSRF 请求头。")
        .add("en", "csrf_mismatch", "CSRF token mismatch.")
        .add("zh", "csrf_mismatch", "CSRF 令牌不匹配，疑似伪造请求。")
        // 请求超时（tibba_middleware::handle_middleware_error）
        .add("en", "timeout", "Request took too long.")
        .add("zh", "timeout", "请求处理超时，请稍后重试。")
        // 认证与权限（tibba-auth）
        .add(
            "en",
            "not_logged_in",
            "Authentication required, please sign in.",
        )
        .add("zh", "not_logged_in", "请先登录后再操作。")
        .add("en", "not_admin", "Administrator privileges required.")
        .add("zh", "not_admin", "需要管理员权限。")
        .add(
            "en",
            "permission_denied",
            "You do not have permission to perform this action.",
        )
        .add("zh", "permission_denied", "当前账号缺少执行该操作的权限。")
        // 幂等与参数校验（tibba-middleware / tibba-model / tibba-router）
        .add(
            "en",
            "idempotency_invalid_key",
            "Invalid Idempotency-Key header.",
        )
        .add(
            "zh",
            "idempotency_invalid_key",
            "幂等键（Idempotency-Key）格式不合法。",
        )
        .add(
            "en",
            "idempotency_conflict",
            "A request with this Idempotency-Key is already in progress.",
        )
        .add(
            "zh",
            "idempotency_conflict",
            "相同幂等键的请求正在处理中，请勿重复提交。",
        )
        .add("en", "validation", "Invalid request parameters.")
        .add("zh", "validation", "请求参数校验未通过。")
        // 登录与账号安全状态（tibba-router）
        .add("en", "bad_credentials", "Incorrect account or password.")
        .add("zh", "bad_credentials", "账号或密码错误。")
        .add(
            "en",
            "too_many_attempts",
            "Too many failed attempts, please try again later.",
        )
        .add("zh", "too_many_attempts", "尝试次数过多，请稍后再试。")
        .add("en", "account_disabled", "This account has been disabled.")
        .add("zh", "account_disabled", "该账号已被禁用。")
        .add(
            "en",
            "old_password_required",
            "Current password is required to change your password.",
        )
        .add(
            "zh",
            "old_password_required",
            "修改密码时必须提供当前密码。",
        )
        .add(
            "en",
            "cannot_unlink_last_login_method",
            "Cannot unlink the last login method; set a password first.",
        )
        .add(
            "zh",
            "cannot_unlink_last_login_method",
            "不能解绑最后一个登录方式，请先设置本地密码。",
        )
        // RBAC / 动态配置 / Metrics（tibba-router / root）
        .add(
            "en",
            "wildcard_reserved",
            "Wildcard permission '*' is reserved for super admins.",
        )
        .add(
            "zh",
            "wildcard_reserved",
            "通配权限 '*' 仅保留给超级管理员，禁止创建或授予。",
        )
        .add(
            "en",
            "wildcard_forbidden",
            "Only super admins can manage wildcard or rbac:write permissions.",
        )
        .add(
            "zh",
            "wildcard_forbidden",
            "仅超级管理员可管理通配权限或 rbac:write 权限。",
        )
        .add(
            "en",
            "super_admin_immutable",
            "Super admin role permissions cannot be modified.",
        )
        .add(
            "zh",
            "super_admin_immutable",
            "超级管理员角色权限不可修改。",
        )
        .add(
            "en",
            "forbidden_category",
            "This configuration category is not publicly accessible.",
        )
        .add("zh", "forbidden_category", "该配置分类未对公开接口开放。")
        .add(
            "en",
            "metrics_unauthorized",
            "Valid metrics token or administrator session is required.",
        )
        .add(
            "zh",
            "metrics_unauthorized",
            "访问指标接口需要有效的采集密钥或管理员会话。",
        )
        // 数据与模型通用错误（tibba-model / tibba-router）
        .add("en", "not_found", "Requested resource was not found.")
        .add("zh", "not_found", "请求的资源不存在。")
        .add("en", "record_not_found", "Record not found.")
        .add("zh", "record_not_found", "未找到对应记录。")
        .add("en", "unique_violation", "Resource already exists.")
        .add("zh", "unique_violation", "资源已存在，请勿重复创建。")
        .add("en", "already_exists", "Resource already exists.")
        .add("zh", "already_exists", "资源已存在。")
        .add(
            "en",
            "foreign_key_violation",
            "Operation blocked by referenced resource constraint.",
        )
        .add(
            "zh",
            "foreign_key_violation",
            "该记录正被其它数据引用，无法执行此操作。",
        )
        .add("en", "constraint_violation", "Invalid field value.")
        .add("zh", "constraint_violation", "字段值不符合约束条件。")
        .add("en", "invalid_input", "Invalid input syntax or value.")
        .add("zh", "invalid_input", "输入参数格式或取值不合法。")
        .add("en", "invalid_datetime", "Invalid date/time format.")
        .add("zh", "invalid_datetime", "日期时间格式不合法。")
        // 用户认证 / 2FA / OAuth 常见错误
        .add(
            "en",
            "token_expired",
            "Login token expired, please try again.",
        )
        .add("zh", "token_expired", "登录令牌已过期，请刷新后重试。")
        .add("en", "user_not_found", "User account no longer exists.")
        .add("zh", "user_not_found", "用户账号不存在或已被删除。")
        .add("en", "bad_code", "Invalid verification or recovery code.")
        .add("zh", "bad_code", "动态码或恢复码错误。")
        .add(
            "en",
            "invalid_challenge",
            "Verification challenge expired, please sign in again.",
        )
        .add(
            "zh",
            "invalid_challenge",
            "二步验证挑战已失效，请重新登录验证。",
        )
        .add(
            "en",
            "invalid_token",
            "Verification token is invalid or expired.",
        )
        .add("zh", "invalid_token", "验证令牌无效或已过期。")
        .add("en", "no_email", "Please bind an email address first.")
        .add("zh", "no_email", "当前账号尚未绑定邮箱。")
        .add(
            "en",
            "email_changed",
            "Email address has changed, please request a new verification.",
        )
        .add(
            "zh",
            "email_changed",
            "邮箱地址已变更，请重新发送验证邮件。",
        )
        .add(
            "en",
            "oauth_link_not_found",
            "Connected OAuth account not found.",
        )
        .add("zh", "oauth_link_not_found", "未找到对应的第三方账号绑定。")
        .add("en", "login_required", "Please sign in first.")
        .add("zh", "login_required", "请先登录后再进行绑定。")
        .add(
            "en",
            "state_invalid",
            "OAuth authorization state expired or invalid.",
        )
        .add(
            "zh",
            "state_invalid",
            "第三方授权状态已过期或无效，请重试。",
        )
        // 管理后台防越权 / RBAC / 文件上传限制
        .add(
            "en",
            "self_operation_forbidden",
            "You cannot disable or delete your own account.",
        )
        .add(
            "zh",
            "self_operation_forbidden",
            "禁止禁用或删除当前登录的自身账号。",
        )
        .add(
            "en",
            "privilege_escalation",
            "Insufficient privilege to manage super admin accounts.",
        )
        .add(
            "zh",
            "privilege_escalation",
            "权限不足，禁止操作超级管理员账号或角色。",
        )
        .add("en", "permission_not_found", "Permission code not found.")
        .add("zh", "permission_not_found", "目标权限码尚未注册。")
        .add(
            "en",
            "invalid_permission_code",
            "Invalid permission code format.",
        )
        .add(
            "zh",
            "invalid_permission_code",
            "权限码格式不合法（仅支持 ASCII 字母数字与 : _ - *）。",
        )
        .add("en", "invalid_role_name", "Invalid role name format.")
        .add(
            "zh",
            "invalid_role_name",
            "角色名格式不合法（仅支持 ASCII 字母数字与 _ -）。",
        )
        .add("en", "invalid_filename", "Invalid file name.")
        .add("zh", "invalid_filename", "文件名包含非法字符或格式不正确。")
        .add(
            "en",
            "payload_too_large",
            "Uploaded file exceeds size limit.",
        )
        .add("zh", "payload_too_large", "上传文件大小超出限制。")
        .add("en", "too_many_files", "Too many files uploaded at once.")
        .add("zh", "too_many_files", "单次上传文件数量超出限制。")
        .add(
            "en",
            "invalid_sql",
            "Invalid SQL query definition or parameter.",
        )
        .add("zh", "invalid_sql", "SQL 查询定义或参数不合法。")
        .add(
            "en",
            "sql_not_readonly",
            "Only single read-only SELECT or WITH SELECT statements are allowed.",
        )
        .add(
            "zh",
            "sql_not_readonly",
            "仅允许单条只读 SELECT 或 WITH ... SELECT 查询语句。",
        )
        .add(
            "en",
            "query_not_found",
            "SQL query endpoint not found or inactive.",
        )
        .add("zh", "query_not_found", "SQL 查询接口不存在或未启用。")
        .add("en", "sql_execution_error", "Failed to execute SQL query.")
        .add("zh", "sql_execution_error", "SQL 查询执行失败。")
        .add("en", "invalid_spec", "Invalid SQL query specification.")
        .add("zh", "invalid_spec", "SQL 查询规格配置不合法。")
        .add("en", "invalid_param", "Invalid query or parameter name.")
        .add("zh", "invalid_param", "查询名称或参数名称格式不合法。")
        .add(
            "en",
            "stored_json",
            "Stored SQL query configuration is corrupted.",
        )
        .add("zh", "stored_json", "存储的 SQL 查询配置格式已损坏。")
        .add(
            "en",
            "group_not_found",
            "SQL query group does not exist or is disabled.",
        )
        .add("zh", "group_not_found", "所选 SQL 查询分组不存在或未启用。")
        .add("en", "group_exists", "SQL query group already exists.")
        .add("zh", "group_exists", "该 SQL 查询分组已存在。")
        .add(
            "en",
            "group_in_use",
            "Cannot delete a group still referenced by SQL queries.",
        )
        .add(
            "zh",
            "group_in_use",
            "该分组下仍有关联的 SQL 查询接口，无法删除。",
        )
        .add(
            "en",
            "app_not_found",
            "Application client not found or inactive.",
        )
        .add("zh", "app_not_found", "应用凭证不存在或已禁用。")
        .add("en", "app_exists", "Application ID already exists.")
        .add("zh", "app_exists", "该应用 ID 已存在。")
        .add("en", "invalid_signature", "Invalid application signature.")
        .add(
            "zh",
            "invalid_signature",
            "应用签名校验未通过或格式不合法。",
        )
        .add(
            "en",
            "signature_expired",
            "Application signature timestamp expired.",
        )
        .add("zh", "signature_expired", "应用签名时间戳已过期。")
        .add(
            "en",
            "app_session_forbidden",
            "Application credentials cannot access personal user endpoints.",
        )
        .add(
            "zh",
            "app_session_forbidden",
            "应用凭证禁止访问个人账号专属接口。",
        )
        .add(
            "en",
            "forbidden_category",
            "This configuration category is restricted.",
        )
        .add(
            "zh",
            "forbidden_category",
            "当前配置分类不允许通过该接口修改或公开读取。",
        );
    let _ = tibba_middleware::i18n::init(catalog);
}
