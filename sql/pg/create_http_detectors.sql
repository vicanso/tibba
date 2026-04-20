CREATE TABLE http_detectors (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  status SMALLINT NOT NULL DEFAULT 0,
  name VARCHAR(255) NOT NULL,
  "interval" SMALLINT NOT NULL DEFAULT 1,
  url TEXT NOT NULL,
  method VARCHAR(10) NOT NULL DEFAULT 'GET',
  alpn_protocols JSONB NOT NULL DEFAULT '[]',
  resolves JSONB NOT NULL DEFAULT '[]',
  headers JSONB NOT NULL DEFAULT '{}',
  ip_version SMALLINT NOT NULL DEFAULT 0,
  skip_verify BOOLEAN NOT NULL DEFAULT FALSE,
  retries SMALLINT NOT NULL DEFAULT 0,
  failure_threshold SMALLINT NOT NULL DEFAULT 0,
  dns_servers JSONB NOT NULL DEFAULT '[]',
  body BYTEA,
  script TEXT,
  alarm_url VARCHAR(1024) NOT NULL DEFAULT '',
  random_querystring BOOLEAN NOT NULL DEFAULT FALSE,
  alarm_on_change BOOLEAN NOT NULL DEFAULT FALSE,
  "verbose" BOOLEAN NOT NULL DEFAULT FALSE,
  regions JSONB NOT NULL DEFAULT '[]',
  group_id BIGINT NOT NULL,
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  created_by BIGINT NOT NULL,
  remark VARCHAR(1000) NOT NULL DEFAULT '',
  modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX name_group_id ON http_detectors (name, group_id, deleted_at);
CREATE INDEX idx_http_detectors_deleted_at ON http_detectors (deleted_at);

CREATE TRIGGER set_http_detectors_modified
  BEFORE UPDATE ON http_detectors
  FOR EACH ROW EXECUTE FUNCTION trigger_set_modified_timestamp();

COMMENT ON TABLE http_detectors IS 'HTTP检测器表';
COMMENT ON COLUMN http_detectors.id IS '主键ID';
COMMENT ON COLUMN http_detectors.status IS '状态，0：禁用，1：启用';
COMMENT ON COLUMN http_detectors.name IS '名称';
COMMENT ON COLUMN http_detectors."interval" IS '间隔时间，单位：分钟';
COMMENT ON COLUMN http_detectors.url IS 'URL';
COMMENT ON COLUMN http_detectors.method IS 'HTTP方法';
COMMENT ON COLUMN http_detectors.alpn_protocols IS 'ALPN协议';
COMMENT ON COLUMN http_detectors.resolves IS 'DNS解析';
COMMENT ON COLUMN http_detectors.headers IS 'HTTP头';
COMMENT ON COLUMN http_detectors.ip_version IS 'IP版本';
COMMENT ON COLUMN http_detectors.skip_verify IS '是否跳过证书验证';
COMMENT ON COLUMN http_detectors.retries IS '重试次数';
COMMENT ON COLUMN http_detectors.failure_threshold IS '失败阈值';
COMMENT ON COLUMN http_detectors.dns_servers IS 'DNS服务器';
COMMENT ON COLUMN http_detectors.body IS '请求体';
COMMENT ON COLUMN http_detectors.script IS '脚本';
COMMENT ON COLUMN http_detectors.alarm_url IS '告警URL';
COMMENT ON COLUMN http_detectors.random_querystring IS '是否添加随机查询字符串';
COMMENT ON COLUMN http_detectors.alarm_on_change IS '是否仅在状态变更时推送告警';
COMMENT ON COLUMN http_detectors."verbose" IS '是否详细输出';
COMMENT ON COLUMN http_detectors.regions IS '触发区域';
COMMENT ON COLUMN http_detectors.group_id IS '组ID';
COMMENT ON COLUMN http_detectors.created IS '创建时间';
COMMENT ON COLUMN http_detectors.created_by IS '创建人';
COMMENT ON COLUMN http_detectors.remark IS '备注';
COMMENT ON COLUMN http_detectors.modified IS '更新时间';
COMMENT ON COLUMN http_detectors.deleted_at IS '软删除时间';
