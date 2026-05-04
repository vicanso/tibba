CREATE TABLE token_prices (
  id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  service      VARCHAR(64)  NOT NULL,
  model        VARCHAR(128) NOT NULL DEFAULT '',
  input_price  BIGINT       NOT NULL DEFAULT 0,
  output_price BIGINT       NOT NULL DEFAULT 0,
  fixed_price  BIGINT       NOT NULL DEFAULT 0,
  unit_size    INT          NOT NULL DEFAULT 1000,
  status       SMALLINT     NOT NULL DEFAULT 1,
  remark       VARCHAR(500) NOT NULL DEFAULT '',
  created      TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
  modified     TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at   TIMESTAMP    DEFAULT NULL
);

CREATE UNIQUE INDEX uk_token_prices_service_model ON token_prices (service, model) WHERE deleted_at IS NULL;

CREATE TRIGGER set_token_prices_modified
  BEFORE UPDATE ON token_prices
  FOR EACH ROW EXECUTE FUNCTION trigger_set_modified_timestamp();

COMMENT ON TABLE token_prices IS '积分定价配置表';
COMMENT ON COLUMN token_prices.id IS '主键ID';
COMMENT ON COLUMN token_prices.service IS '服务类型：llm、api等';
COMMENT ON COLUMN token_prices.model IS '模型名称，通用API场景为空字符串';
COMMENT ON COLUMN token_prices.input_price IS '每unit_size个输入token扣除的积分数';
COMMENT ON COLUMN token_prices.output_price IS '每unit_size个输出token扣除的积分数';
COMMENT ON COLUMN token_prices.fixed_price IS '每次调用固定扣除积分数';
COMMENT ON COLUMN token_prices.unit_size IS '计费基数，默认1000（即per 1K tokens）';
COMMENT ON COLUMN token_prices.status IS '状态，1：启用，0：禁用';
COMMENT ON COLUMN token_prices.remark IS '备注';
COMMENT ON COLUMN token_prices.created IS '创建时间';
COMMENT ON COLUMN token_prices.modified IS '更新时间';
COMMENT ON COLUMN token_prices.deleted_at IS '软删除时间';
