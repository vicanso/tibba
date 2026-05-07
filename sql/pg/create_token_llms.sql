CREATE TABLE token_llms (
  id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  name       VARCHAR(100) NOT NULL,
  url        VARCHAR(500) NOT NULL,
  model      VARCHAR(128) NOT NULL,
  api_key    VARCHAR(500) NOT NULL,
  provider   VARCHAR(20)  NOT NULL DEFAULT 'openai',
  status     SMALLINT     NOT NULL DEFAULT 1,
  remark     VARCHAR(500) NOT NULL DEFAULT '',
  created    TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
  modified   TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMP    DEFAULT NULL
);

CREATE UNIQUE INDEX uk_token_llms_name ON token_llms (name) WHERE deleted_at IS NULL;

COMMENT ON TABLE token_llms IS 'LLM 服务配置表';
COMMENT ON COLUMN token_llms.id      IS '主键ID';
COMMENT ON COLUMN token_llms.name    IS '配置名称（唯一），如 default、premium 等';
COMMENT ON COLUMN token_llms.url     IS 'LLM API base URL';
COMMENT ON COLUMN token_llms.model   IS '模型名（与 token_prices.model 对应用于计费）';
COMMENT ON COLUMN token_llms.api_key IS 'LLM API 密钥';
COMMENT ON COLUMN token_llms.provider IS '后端协议：openai（默认）或 anthropic';
COMMENT ON COLUMN token_llms.status  IS '状态，1：启用，0：禁用';
COMMENT ON COLUMN token_llms.remark  IS '备注';
COMMENT ON COLUMN token_llms.created IS '创建时间';
COMMENT ON COLUMN token_llms.modified IS '更新时间';
COMMENT ON COLUMN token_llms.deleted_at IS '软删除时间';
