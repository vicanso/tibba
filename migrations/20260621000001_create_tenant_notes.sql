-- 行级多租户演示表（tibba-tenant）。
--
-- 用途：示范「共享表 + tenant_id 列」的行级多租户隔离模式——每条数据带 tenant_id，
-- 所有读写恒按它过滤；增删改用 `id = $1 AND tenant_id = $2` 做纵深防御，即便猜到别的
-- 租户的行 id 也无法越权。不改动任何现有业务表，仅作模式演示（见 src/tenant.rs）。
--
-- 字段语义：
-- - tenant_id  租户标识（隔离键），由请求的 X-Tenant-Id 头 / JWT claim 解析而来
-- - content    便签内容
CREATE TABLE IF NOT EXISTS tenant_notes (
  id        BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  tenant_id VARCHAR(64) NOT NULL,
  content   TEXT        NOT NULL,
  created   TIMESTAMP   NOT NULL DEFAULT now()
);

-- 列表热点：按 (tenant_id, id DESC) 建复合索引，覆盖「某租户最近 N 条」的查询
CREATE INDEX IF NOT EXISTS idx_tenant_notes_tenant ON tenant_notes (tenant_id, id DESC);
