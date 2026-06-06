CREATE TABLE audit_logs (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  user_id BIGINT,
  action VARCHAR(64) NOT NULL,
  target_type VARCHAR(64) NOT NULL DEFAULT '',
  target_id VARCHAR(64) NOT NULL DEFAULT '',
  detail JSONB NOT NULL DEFAULT '{}'::jsonb,
  request_id VARCHAR(128) NOT NULL DEFAULT '',
  ip VARCHAR(64) NOT NULL DEFAULT '',
  user_agent VARCHAR(255) NOT NULL DEFAULT '',
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_audit_logs_user ON audit_logs (user_id, created DESC);
CREATE INDEX idx_audit_logs_action ON audit_logs (action, created DESC);
CREATE INDEX idx_audit_logs_request ON audit_logs (request_id);
CREATE INDEX idx_audit_logs_created ON audit_logs (created DESC);

COMMENT ON TABLE audit_logs IS '审计日志：记录关键操作的「谁、何时、做了什么」';
COMMENT ON COLUMN audit_logs.id IS '主键ID';
COMMENT ON COLUMN audit_logs.user_id IS '操作主体；NULL 表示匿名 / 系统级操作';
COMMENT ON COLUMN audit_logs.action IS '操作类型，约定 "{resource}.{action}" 形如 user.login';
COMMENT ON COLUMN audit_logs.target_type IS '操作目标类型，如 user / file / permission';
COMMENT ON COLUMN audit_logs.target_id IS '操作目标 id 字符串';
COMMENT ON COLUMN audit_logs.detail IS '自由结构化补充信息（前后值、provider、命中规则等）';
COMMENT ON COLUMN audit_logs.request_id IS '关联 X-Request-ID，串联请求链路';
COMMENT ON COLUMN audit_logs.ip IS '客户端 IP（取自 ClientIp 提取器）';
COMMENT ON COLUMN audit_logs.user_agent IS 'User-Agent 截断 255 字符';
COMMENT ON COLUMN audit_logs.created IS '创建时间';
