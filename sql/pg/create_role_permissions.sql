CREATE TABLE role_permissions (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  role VARCHAR(64) NOT NULL,
  permission_code VARCHAR(100) NOT NULL,
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX uk_role_permission ON role_permissions (role, permission_code) WHERE deleted_at IS NULL;
CREATE INDEX idx_role_permissions_role ON role_permissions (role, deleted_at);
CREATE INDEX idx_role_permissions_deleted_at ON role_permissions (deleted_at);

COMMENT ON TABLE role_permissions IS '角色到权限的多对多映射表';
COMMENT ON COLUMN role_permissions.id IS '主键ID';
COMMENT ON COLUMN role_permissions.role IS '角色名，与 users.roles 中存储的字符串保持一致（如 "su"、"admin"）';
COMMENT ON COLUMN role_permissions.permission_code IS '权限码，与 permissions.code 对应；不加外键以支持通配权限码';
COMMENT ON COLUMN role_permissions.created IS '创建时间';
COMMENT ON COLUMN role_permissions.deleted_at IS '软删除时间';

-- 种子数据：登记通配权限码并授予超级管理员角色
INSERT INTO permissions (code, description)
VALUES ('*', 'Wildcard — grants every action')
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role, permission_code)
VALUES ('su', '*')
ON CONFLICT DO NOTHING;
