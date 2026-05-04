CREATE TABLE token_usages (
  id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  user_id       BIGINT       NOT NULL,
  service       VARCHAR(64)  NOT NULL,
  amount        BIGINT       NOT NULL,
  model         VARCHAR(128) NOT NULL DEFAULT '',
  input_tokens  INT          NOT NULL DEFAULT 0,
  output_tokens INT          NOT NULL DEFAULT 0,
  api_path      VARCHAR(256) NOT NULL DEFAULT '',
  duration_ms   INT          NOT NULL DEFAULT 0,
  biz_id        VARCHAR(128) NOT NULL DEFAULT '',
  remark        VARCHAR(500) NOT NULL DEFAULT '',
  created       TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
  modified      TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at    TIMESTAMP    DEFAULT NULL
);

CREATE INDEX idx_token_usages_user    ON token_usages (user_id, created);
CREATE INDEX idx_token_usages_service ON token_usages (service, model, created);
CREATE INDEX idx_token_usages_biz     ON token_usages (biz_id) WHERE biz_id <> '';

CREATE TRIGGER set_token_usages_modified
  BEFORE UPDATE ON token_usages
  FOR EACH ROW EXECUTE FUNCTION trigger_set_modified_timestamp();

COMMENT ON TABLE token_usages IS '积分消耗记录表';
COMMENT ON COLUMN token_usages.id IS '主键ID';
COMMENT ON COLUMN token_usages.user_id IS '用户ID';
COMMENT ON COLUMN token_usages.service IS '服务类型：llm、api、storage等';
COMMENT ON COLUMN token_usages.amount IS '本次扣除积分数';
COMMENT ON COLUMN token_usages.model IS 'LLM模型名称，非LLM场景为空';
COMMENT ON COLUMN token_usages.input_tokens IS '输入token数，非LLM场景为0';
COMMENT ON COLUMN token_usages.output_tokens IS '输出token数，非LLM场景为0';
COMMENT ON COLUMN token_usages.api_path IS 'API路径，通用API场景使用';
COMMENT ON COLUMN token_usages.duration_ms IS '调用耗时（毫秒）';
COMMENT ON COLUMN token_usages.biz_id IS '关联业务ID（请求ID、任务ID等）';
COMMENT ON COLUMN token_usages.remark IS '备注';
COMMENT ON COLUMN token_usages.created IS '创建时间';
COMMENT ON COLUMN token_usages.modified IS '更新时间';
COMMENT ON COLUMN token_usages.deleted_at IS '软删除时间';
