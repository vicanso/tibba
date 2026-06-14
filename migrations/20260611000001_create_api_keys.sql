-- 用户 API Key / 个人访问令牌（PAT）表。
--
-- 用于「机器对机器」鉴权（CI / 脚本 / 第三方集成），区别于浏览器 Cookie Session
-- 与短期 JWT。令牌明文仅在创建接口返回一次，库中只存其 SHA-256 哈希（key_hash），
-- 无法反推；丢失只能吊销重建。
--
-- 字段语义：
-- - name          用户自定义标签，便于在个人中心区分用途（"ci"、"backup-script"）
-- - key_prefix    令牌前若干字符（如 "tibba_a1b2c3d4"），仅供展示/识别，不参与鉴权
-- - key_hash      sha256(完整令牌) 的十六进制串；鉴权时按此列命中
-- - last_used_at  最近一次成功鉴权时间，便于发现长期不用 / 可疑活跃的 key
-- - expires_at    过期时间；NULL 表示永不过期
-- - deleted_at    软删除（吊销）时间；NULL 表示有效
CREATE TABLE IF NOT EXISTS api_keys (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  user_id BIGINT NOT NULL,
  name VARCHAR(128) NOT NULL DEFAULT '',
  key_prefix VARCHAR(32) NOT NULL,
  key_hash VARCHAR(128) NOT NULL,
  last_used_at TIMESTAMP DEFAULT NULL,
  expires_at TIMESTAMP DEFAULT NULL,
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMP DEFAULT NULL
);

-- 鉴权热点：按 key_hash 唯一命中。哈希本身随机不复用，软删除记录保留其哈希无妨。
CREATE UNIQUE INDEX IF NOT EXISTS uk_api_keys_hash ON api_keys (key_hash);

-- 个人中心：按 user_id 反查自己的 key 列表（含软删除维度）。
CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys (user_id, deleted_at);
