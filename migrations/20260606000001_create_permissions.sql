-- 权限点表：每条记录代表一个原子权限码，如 "user:read"、"file:delete"、"*"。
-- 这里只登记"权限存在"，不绑定具体用户或角色——角色到权限的多对多关系在 role_permissions 表。
-- 列类型 / 索引名与 sql/pg/create_permissions.sql 完全一致，两份 schema 保持双向可替换。
CREATE TABLE IF NOT EXISTS permissions (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  -- 权限码：建议形如 "resource:action"；"*" 为通配符；"resource:*" 为前缀通配
  code VARCHAR(100) NOT NULL,
  -- 给运维/管理面板看的描述文案
  description VARCHAR(255) NOT NULL DEFAULT '',
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  -- 软删除时间戳，NULL 表示生效
  deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS uk_permissions_code ON permissions (code, deleted_at);
CREATE INDEX IF NOT EXISTS idx_permissions_deleted_at ON permissions (deleted_at);
