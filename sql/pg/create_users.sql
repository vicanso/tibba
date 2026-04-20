CREATE TABLE users (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  status SMALLINT NOT NULL DEFAULT 0,
  account VARCHAR(255) NOT NULL,
  password VARCHAR(255) NOT NULL,
  roles JSONB NOT NULL DEFAULT '[]',
  "groups" JSONB NOT NULL DEFAULT '[]',
  remark VARCHAR(255) NOT NULL DEFAULT '',
  email VARCHAR(255) NOT NULL DEFAULT '',
  avatar VARCHAR(1024) NOT NULL DEFAULT '',
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX user_account ON users (account, deleted_at);
CREATE INDEX idx_users_deleted_at ON users (deleted_at);

CREATE TRIGGER set_users_modified
  BEFORE UPDATE ON users
  FOR EACH ROW EXECUTE FUNCTION trigger_set_modified_timestamp();

COMMENT ON TABLE users IS '用户表';
COMMENT ON COLUMN users.id IS '主键ID';
COMMENT ON COLUMN users.status IS '状态，0：禁用，1：启用';
COMMENT ON COLUMN users.password IS '密码';
COMMENT ON COLUMN users.roles IS '用户角色';
COMMENT ON COLUMN users."groups" IS '用户群组';
COMMENT ON COLUMN users.remark IS '备注';
COMMENT ON COLUMN users.email IS '用户邮箱';
COMMENT ON COLUMN users.avatar IS '用户头像';
COMMENT ON COLUMN users.created IS '创建时间';
COMMENT ON COLUMN users.modified IS '更新时间';
COMMENT ON COLUMN users.deleted_at IS '软删除时间';
