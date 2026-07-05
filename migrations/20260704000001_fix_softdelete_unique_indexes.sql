-- 修复软删除唯一约束：原索引把 deleted_at 纳入索引列。
--
-- 因 Postgres 唯一索引中 NULL <> NULL，活跃行（deleted_at IS NULL）之间的唯一性实际
-- 未被强制，可静默插入重复；且 `ON CONFLICT (col)` 无法匹配含 deleted_at 的复合索引，
-- 首次执行即报 42P10。改为「仅活跃行」的部分唯一索引：唯一性对活跃行生效，软删除历史行
-- 不受约束可共存。与 token_* 系列表已有的写法保持一致。

DROP INDEX IF EXISTS uk_permissions_code;
CREATE UNIQUE INDEX IF NOT EXISTS uk_permissions_code
    ON permissions (code) WHERE deleted_at IS NULL;

DROP INDEX IF EXISTS uk_role_permission;
CREATE UNIQUE INDEX IF NOT EXISTS uk_role_permission
    ON role_permissions (role, permission_code) WHERE deleted_at IS NULL;

DROP INDEX IF EXISTS uk_user_oauth_links_provider_uid;
CREATE UNIQUE INDEX IF NOT EXISTS uk_user_oauth_links_provider_uid
    ON user_oauth_links (provider, provider_user_id) WHERE deleted_at IS NULL;
