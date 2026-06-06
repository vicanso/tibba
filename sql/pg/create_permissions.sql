CREATE TABLE permissions (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  code VARCHAR(100) NOT NULL,
  description VARCHAR(255) NOT NULL DEFAULT '',
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX uk_permissions_code ON permissions (code, deleted_at);
CREATE INDEX idx_permissions_deleted_at ON permissions (deleted_at);

COMMENT ON TABLE permissions IS '权限点表，登记 RBAC 中所有可被授予的原子权限码';
COMMENT ON COLUMN permissions.id IS '主键ID';
COMMENT ON COLUMN permissions.code IS '权限码，形如 "resource:action"；"*" 为通配；"resource:*" 为前缀通配';
COMMENT ON COLUMN permissions.description IS '给运维/管理面板的描述文案';
COMMENT ON COLUMN permissions.created IS '创建时间';
COMMENT ON COLUMN permissions.modified IS '更新时间';
COMMENT ON COLUMN permissions.deleted_at IS '软删除时间';
