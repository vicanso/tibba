-- 审计日志表：记录「谁在何时通过哪个请求做了什么」。
-- 设计原则：handler 关键操作显式调用 `AuditLogModel::log(...)`，中间件不做自动记录
-- （Phase 1 Q3 选择 ii：业务侧显式，覆盖更精准、避免泛 GET 噪音）。
--
-- 检索模式：
-- - 按 user 看审计史 → idx_audit_logs_user
-- - 按 action 全量看动作 → idx_audit_logs_action
-- - 按 request_id 关联完整请求链路（含日志 / 错误） → idx_audit_logs_request
CREATE TABLE IF NOT EXISTS audit_logs (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  -- 操作主体；NULL = 匿名 / 系统级操作（如未登录请求重置密码）
  user_id BIGINT,
  -- 操作类型：约定 "{resource}.{action}" 形如 "user.login" / "user.password_reset"
  action VARCHAR(64) NOT NULL,
  -- 操作目标类型与 id；用于关联具体业务实体（如改了哪个 file / 哪个 detector）
  target_type VARCHAR(64) NOT NULL DEFAULT '',
  target_id VARCHAR(64) NOT NULL DEFAULT '',
  -- 自由结构化补充信息（前后值、命中规则、provider 等）
  detail JSONB NOT NULL DEFAULT '{}'::jsonb,
  -- 与 Request ID 中间件关联，便于把审计行 + 应用日志 + 错误堆栈关联起来
  request_id VARCHAR(128) NOT NULL DEFAULT '',
  -- 客户端 IP（取自 ClientIp 提取器，已处理 X-Forwarded-For）
  ip VARCHAR(64) NOT NULL DEFAULT '',
  -- User-Agent 截断 255 字符防超长
  user_agent VARCHAR(255) NOT NULL DEFAULT '',
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_audit_logs_user
  ON audit_logs (user_id, created DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_action
  ON audit_logs (action, created DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_request
  ON audit_logs (request_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_created
  ON audit_logs (created DESC);
