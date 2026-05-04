CREATE TABLE token_accounts (
  id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  user_id        BIGINT    NOT NULL,
  balance        BIGINT    NOT NULL DEFAULT 0,
  total_recharged BIGINT   NOT NULL DEFAULT 0,
  total_consumed  BIGINT   NOT NULL DEFAULT 0,
  status         SMALLINT  NOT NULL DEFAULT 1,
  remark         VARCHAR(500) NOT NULL DEFAULT '',
  created        TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  modified       TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at     TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX uk_token_accounts_user ON token_accounts (user_id) WHERE deleted_at IS NULL;
CREATE INDEX idx_token_accounts_deleted_at ON token_accounts (deleted_at);

CREATE TRIGGER set_token_accounts_modified
  BEFORE UPDATE ON token_accounts
  FOR EACH ROW EXECUTE FUNCTION trigger_set_modified_timestamp();

COMMENT ON TABLE token_accounts IS '积分账户表';
COMMENT ON COLUMN token_accounts.id IS '主键ID';
COMMENT ON COLUMN token_accounts.user_id IS '用户ID';
COMMENT ON COLUMN token_accounts.balance IS '当前可用积分';
COMMENT ON COLUMN token_accounts.total_recharged IS '历史累计充值积分';
COMMENT ON COLUMN token_accounts.total_consumed IS '历史累计消费积分';
COMMENT ON COLUMN token_accounts.status IS '账户状态，1：正常，0：冻结';
COMMENT ON COLUMN token_accounts.remark IS '备注';
COMMENT ON COLUMN token_accounts.created IS '创建时间';
COMMENT ON COLUMN token_accounts.modified IS '更新时间';
COMMENT ON COLUMN token_accounts.deleted_at IS '软删除时间';
