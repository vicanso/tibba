CREATE TABLE token_keys (
  id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  user_id     BIGINT        NOT NULL,
  token       VARCHAR(64)   NOT NULL,
  name        VARCHAR(100)  NOT NULL DEFAULT '',
  status      SMALLINT      NOT NULL DEFAULT 1,
  expired_at  TIMESTAMP     DEFAULT NULL,
  created_by  BIGINT        NOT NULL DEFAULT 0,
  created     TIMESTAMP     NOT NULL DEFAULT CURRENT_TIMESTAMP,
  modified    TIMESTAMP     NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at  TIMESTAMP     DEFAULT NULL
);

CREATE UNIQUE INDEX uk_token_keys_token ON token_keys (token) WHERE deleted_at IS NULL;
CREATE INDEX idx_token_keys_user_id ON token_keys (user_id);
CREATE INDEX idx_token_keys_deleted_at ON token_keys (deleted_at);

CREATE TRIGGER set_token_keys_modified
  BEFORE UPDATE ON token_keys
  FOR EACH ROW EXECUTE FUNCTION trigger_set_modified_timestamp();

COMMENT ON TABLE token_keys IS 'API 鉴权密钥表';
COMMENT ON COLUMN token_keys.id IS '主键ID';
COMMENT ON COLUMN token_keys.user_id IS '关联用户ID';
COMMENT ON COLUMN token_keys.token IS 'API 密钥（UUID v4）';
COMMENT ON COLUMN token_keys.name IS '密钥备注名称';
COMMENT ON COLUMN token_keys.status IS '状态，1：启用，0：禁用';
COMMENT ON COLUMN token_keys.expired_at IS '过期时间，NULL 表示永不过期';
COMMENT ON COLUMN token_keys.created_by IS '创建人用户ID';
COMMENT ON COLUMN token_keys.created IS '创建时间';
COMMENT ON COLUMN token_keys.modified IS '更新时间';
COMMENT ON COLUMN token_keys.deleted_at IS '软删除时间';
