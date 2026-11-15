-- Tibba 脚手架初始化基线迁移（合并全部基础表与部分唯一索引）
-- 仅包含通用脚手架能力所需的 9 张表：
-- users / configurations / files / permissions / role_permissions /
-- user_oauth_links / api_keys / audit_logs / jobs

-- =============================================================================
-- 1. 用户表（支持软删除后复用账号与已验证邮箱）
-- =============================================================================
CREATE TABLE IF NOT EXISTS users (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    status SMALLINT NOT NULL DEFAULT 0,
    account VARCHAR(255) NOT NULL,
    password VARCHAR(255) NOT NULL,
    roles JSONB NOT NULL DEFAULT '[]',
    "groups" JSONB NOT NULL DEFAULT '[]',
    nickname VARCHAR(100) DEFAULT NULL,
    phone VARCHAR(64) DEFAULT NULL,
    remark VARCHAR(255) NOT NULL DEFAULT '',
    email VARCHAR(255) NOT NULL DEFAULT '',
    avatar VARCHAR(1024) NOT NULL DEFAULT '',
    last_login_at TIMESTAMP DEFAULT NULL,
    email_verified_at TIMESTAMP DEFAULT NULL,
    totp_secret TEXT DEFAULT NULL,
    totp_enabled_at TIMESTAMP DEFAULT NULL,
    totp_recovery_codes JSONB DEFAULT NULL,
    created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS user_account
    ON users (account)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_users_deleted_at
    ON users (deleted_at);

CREATE INDEX IF NOT EXISTS idx_users_verified_email
    ON users (email)
    WHERE email_verified_at IS NOT NULL AND deleted_at IS NULL;

COMMENT ON TABLE users IS '用户表';
COMMENT ON COLUMN users.status IS '状态：0=正常，其他见应用定义';
COMMENT ON COLUMN users.account IS '登录账号';
COMMENT ON COLUMN users.password IS 'Argon2id 密码哈希';
COMMENT ON COLUMN users.roles IS '角色列表（JSON 数组）';
COMMENT ON COLUMN users."groups" IS '用户组列表（JSON 数组）';
COMMENT ON COLUMN users.email_verified_at IS '邮箱验证时间';
COMMENT ON COLUMN users.totp_secret IS '加密后的 TOTP 密钥';
COMMENT ON COLUMN users.totp_enabled_at IS 'TOTP 双因素认证启用时间';
COMMENT ON COLUMN users.totp_recovery_codes IS 'TOTP 恢复码哈希列表（JSON 数组）';
COMMENT ON COLUMN users.deleted_at IS '软删除时间';

-- =============================================================================
-- 2. 动态配置表
-- =============================================================================
CREATE TABLE IF NOT EXISTS configurations (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    status SMALLINT NOT NULL DEFAULT 0,
    category VARCHAR(50) NOT NULL,
    name VARCHAR(100) NOT NULL,
    data JSONB NOT NULL,
    description VARCHAR(255) NOT NULL DEFAULT '',
    effective_start_time TIMESTAMP NOT NULL,
    effective_end_time TIMESTAMP NOT NULL,
    created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS uk_category_name
    ON configurations (category, name)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_configurations_effective_time
    ON configurations (status, effective_start_time, effective_end_time, deleted_at);

COMMENT ON TABLE configurations IS '动态配置表';
COMMENT ON COLUMN configurations.category IS '配置分类';
COMMENT ON COLUMN configurations.name IS '配置名称';
COMMENT ON COLUMN configurations.data IS '配置 JSON 内容';
COMMENT ON COLUMN configurations.deleted_at IS '软删除时间';

-- =============================================================================
-- 3. 文件元数据表
-- =============================================================================
CREATE TABLE IF NOT EXISTS files (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    filename VARCHAR(255) NOT NULL,
    file_size BIGINT NOT NULL,
    content_type VARCHAR(100) NOT NULL,
    "group" VARCHAR(100) NOT NULL,
    uploader VARCHAR(100) NOT NULL,
    image_width INTEGER NOT NULL DEFAULT -1,
    image_height INTEGER NOT NULL DEFAULT -1,
    metadata JSONB NOT NULL DEFAULT '{}',
    created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS file_name
    ON files (filename)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_files_deleted_at
    ON files (deleted_at);

COMMENT ON TABLE files IS '上传文件元数据表';
COMMENT ON COLUMN files.filename IS '存储对象路径/文件名';
COMMENT ON COLUMN files."group" IS '文件分组';
COMMENT ON COLUMN files.deleted_at IS '软删除时间';

-- =============================================================================
-- 4. RBAC 权限表与角色-权限关联表
-- =============================================================================
CREATE TABLE IF NOT EXISTS permissions (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    code VARCHAR(100) NOT NULL,
    description VARCHAR(255) NOT NULL DEFAULT '',
    created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS uk_permissions_code
    ON permissions (code)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_permissions_deleted_at
    ON permissions (deleted_at);

CREATE TABLE IF NOT EXISTS role_permissions (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    role VARCHAR(64) NOT NULL,
    permission_code VARCHAR(100) NOT NULL,
    created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS uk_role_permission
    ON role_permissions (role, permission_code)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_role_permissions_role
    ON role_permissions (role, deleted_at);

CREATE INDEX IF NOT EXISTS idx_role_permissions_deleted_at
    ON role_permissions (deleted_at);

-- 默认种子：超级管理员角色 su 拥有通配符权限 *
INSERT INTO permissions (code, description)
VALUES ('*', 'Wildcard — grants every action')
ON CONFLICT (code) WHERE deleted_at IS NULL DO NOTHING;

INSERT INTO role_permissions (role, permission_code)
VALUES ('su', '*')
ON CONFLICT (role, permission_code) WHERE deleted_at IS NULL DO NOTHING;

-- =============================================================================
-- 5. OAuth 账号绑定表
-- =============================================================================
CREATE TABLE IF NOT EXISTS user_oauth_links (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id BIGINT NOT NULL,
    provider VARCHAR(32) NOT NULL,
    provider_user_id VARCHAR(64) NOT NULL,
    provider_login VARCHAR(255) NOT NULL DEFAULT '',
    provider_email VARCHAR(255) NOT NULL DEFAULT '',
    created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS uk_user_oauth_links_provider_uid
    ON user_oauth_links (provider, provider_user_id)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_user_oauth_links_user
    ON user_oauth_links (user_id, deleted_at);

CREATE INDEX IF NOT EXISTS idx_user_oauth_links_deleted_at
    ON user_oauth_links (deleted_at);

-- =============================================================================
-- 6. API Key 表
-- =============================================================================
CREATE TABLE IF NOT EXISTS api_keys (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id BIGINT NOT NULL,
    name VARCHAR(128) NOT NULL DEFAULT '',
    key_prefix VARCHAR(32) NOT NULL,
    key_hash VARCHAR(128) NOT NULL,
    last_used_at TIMESTAMP DEFAULT NULL,
    expires_at TIMESTAMP DEFAULT NULL,
    created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS uk_api_keys_hash
    ON api_keys (key_hash);

CREATE INDEX IF NOT EXISTS idx_api_keys_user
    ON api_keys (user_id, deleted_at);

-- =============================================================================
-- 7. 审计日志表（仅追加）
-- =============================================================================
CREATE TABLE IF NOT EXISTS audit_logs (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id BIGINT,
    action VARCHAR(64) NOT NULL,
    target_type VARCHAR(64) NOT NULL DEFAULT '',
    target_id VARCHAR(64) NOT NULL DEFAULT '',
    detail JSONB NOT NULL DEFAULT '{}'::jsonb,
    request_id VARCHAR(128) NOT NULL DEFAULT '',
    ip VARCHAR(64) NOT NULL DEFAULT '',
    user_agent VARCHAR(255) NOT NULL DEFAULT '',
    created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_audit_logs_user ON audit_logs (user_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_action ON audit_logs (action);
CREATE INDEX IF NOT EXISTS idx_audit_logs_request ON audit_logs (request_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_created ON audit_logs (created);

-- =============================================================================
-- 8. PG 异步任务队列表
-- =============================================================================
CREATE TABLE IF NOT EXISTS jobs (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    queue VARCHAR(64) NOT NULL DEFAULT 'default',
    job_type VARCHAR(128) NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    status SMALLINT NOT NULL DEFAULT 0,
    attempts INT NOT NULL DEFAULT 0,
    max_attempts INT NOT NULL DEFAULT 25,
    run_at TIMESTAMP NOT NULL DEFAULT now(),
    locked_at TIMESTAMP DEFAULT NULL,
    locked_by VARCHAR(64) DEFAULT NULL,
    last_error TEXT DEFAULT NULL,
    created TIMESTAMP NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_jobs_claim
    ON jobs (run_at)
    WHERE status = 0;

CREATE INDEX IF NOT EXISTS idx_jobs_reap
    ON jobs (locked_at)
    WHERE status = 1;
