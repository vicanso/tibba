-- 角色到权限的多对多映射表。
-- 用户的 roles（users.roles JSONB）→ 本表多条记录 → permissions.code。
-- 运行期对每条 role 取并集即为该用户拥有的全部权限码。
-- 列类型 / 索引名与 sql/pg/create_role_permissions.sql 完全一致。
CREATE TABLE IF NOT EXISTS role_permissions (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  -- 角色名，与 users.roles 中存储的字符串保持一致（如 "su"、"admin"）
  role VARCHAR(64) NOT NULL,
  -- 权限码，与 permissions.code 对应；这里不加外键，是为支持通配权限码的灵活授予
  permission_code VARCHAR(100) NOT NULL,
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  -- 软删除时间戳，NULL 表示生效
  deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS uk_role_permission ON role_permissions (role, permission_code, deleted_at);
CREATE INDEX IF NOT EXISTS idx_role_permissions_role ON role_permissions (role, deleted_at);
CREATE INDEX IF NOT EXISTS idx_role_permissions_deleted_at ON role_permissions (deleted_at);
