CREATE TABLE http_stats (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  target_id BIGINT NOT NULL,
  target_name VARCHAR(255) NOT NULL DEFAULT '',
  url TEXT NOT NULL,
  dns_lookup INTEGER NOT NULL DEFAULT -1,
  quic_connect INTEGER NOT NULL DEFAULT -1,
  tcp_connect INTEGER NOT NULL DEFAULT -1,
  tls_handshake INTEGER NOT NULL DEFAULT -1,
  server_processing INTEGER NOT NULL DEFAULT -1,
  content_transfer INTEGER NOT NULL DEFAULT -1,
  total INTEGER NOT NULL DEFAULT -1,
  addr VARCHAR(255) NOT NULL DEFAULT '',
  status_code SMALLINT NOT NULL DEFAULT 0,
  tls VARCHAR(20) NOT NULL DEFAULT '',
  alpn VARCHAR(10) NOT NULL DEFAULT '',
  subject VARCHAR(1000) NOT NULL DEFAULT '',
  issuer VARCHAR(1000) NOT NULL DEFAULT '',
  cert_not_before VARCHAR(32) NOT NULL DEFAULT '',
  cert_not_after VARCHAR(32) NOT NULL DEFAULT '',
  cert_cipher VARCHAR(50) NOT NULL DEFAULT '',
  cert_domains VARCHAR(3000) NOT NULL DEFAULT '',
  body_size INTEGER NOT NULL DEFAULT -1,
  region VARCHAR(64) NOT NULL DEFAULT '',
  error TEXT,
  result SMALLINT NOT NULL DEFAULT 0,
  remark VARCHAR(1000) NOT NULL DEFAULT '',
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMP DEFAULT NULL
);

CREATE INDEX idx_http_stats_deleted_at ON http_stats (deleted_at);
CREATE INDEX idx_target_id_result ON http_stats (target_id, result);
CREATE INDEX idx_http_stats_modified ON http_stats (modified);

CREATE TRIGGER set_http_stats_modified
  BEFORE UPDATE ON http_stats
  FOR EACH ROW EXECUTE FUNCTION trigger_set_modified_timestamp();

COMMENT ON TABLE http_stats IS 'HTTP统计表';
COMMENT ON COLUMN http_stats.id IS '主键ID';
COMMENT ON COLUMN http_stats.target_id IS '目标ID';
COMMENT ON COLUMN http_stats.target_name IS '目标名称';
COMMENT ON COLUMN http_stats.url IS 'URL';
COMMENT ON COLUMN http_stats.dns_lookup IS 'DNS查询时间';
COMMENT ON COLUMN http_stats.quic_connect IS 'QUIC连接时间';
COMMENT ON COLUMN http_stats.tcp_connect IS 'TCP连接时间';
COMMENT ON COLUMN http_stats.tls_handshake IS 'TLS握手时间';
COMMENT ON COLUMN http_stats.server_processing IS '服务器处理时间';
COMMENT ON COLUMN http_stats.content_transfer IS '内容传输时间';
COMMENT ON COLUMN http_stats.total IS '总时间';
COMMENT ON COLUMN http_stats.addr IS '地址';
COMMENT ON COLUMN http_stats.status_code IS '状态码';
COMMENT ON COLUMN http_stats.tls IS 'TLS版本';
COMMENT ON COLUMN http_stats.alpn IS 'ALPN';
COMMENT ON COLUMN http_stats.subject IS '证书主题';
COMMENT ON COLUMN http_stats.issuer IS '证书颁发者';
COMMENT ON COLUMN http_stats.cert_not_before IS '证书有效期开始时间';
COMMENT ON COLUMN http_stats.cert_not_after IS '证书有效期结束时间';
COMMENT ON COLUMN http_stats.cert_cipher IS '证书加密套件';
COMMENT ON COLUMN http_stats.cert_domains IS '证书域名';
COMMENT ON COLUMN http_stats.body_size IS '响应体大小';
COMMENT ON COLUMN http_stats.region IS '触发区域';
COMMENT ON COLUMN http_stats.error IS '错误信息';
COMMENT ON COLUMN http_stats.result IS '结果';
COMMENT ON COLUMN http_stats.remark IS '备注';
COMMENT ON COLUMN http_stats.created IS '创建时间';
COMMENT ON COLUMN http_stats.modified IS '更新时间';
COMMENT ON COLUMN http_stats.deleted_at IS '软删除时间';
