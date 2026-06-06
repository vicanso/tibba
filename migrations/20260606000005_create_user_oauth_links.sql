-- 用户与第三方身份提供商（GitHub / Google / ...）的关联表。
-- 一个本地 user 可关联多个 provider（同一个 provider 至多一条 active 记录）；
-- 单条 link 唯一标识 = (provider, provider_user_id)——例如 ("github", "12345")。
--
-- 字段语义：
-- - provider           当前仅 "github"；预留 "google"、"apple" 等
-- - provider_user_id   第三方用户的稳定 id（GitHub 数字 id；Google 的 "sub" 字段）
-- - provider_login     展示用的 username / login，第三方可改，仅快照
-- - provider_email     第三方 primary verified email 快照；后续以 users.email 为准
CREATE TABLE IF NOT EXISTS user_oauth_links (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  user_id BIGINT NOT NULL,
  provider VARCHAR(32) NOT NULL,
  provider_user_id VARCHAR(64) NOT NULL,
  provider_login VARCHAR(255) NOT NULL DEFAULT '',
  provider_email VARCHAR(255) NOT NULL DEFAULT '',
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMP DEFAULT NULL
);

-- (provider, provider_user_id) 在未删除态下唯一——防止同一 GitHub 账号被绑到多个本地 user
CREATE UNIQUE INDEX IF NOT EXISTS uk_user_oauth_links_provider_uid
  ON user_oauth_links (provider, provider_user_id, deleted_at);

-- 按 user_id 反向查（用户解绑 / 个人中心展示已绑定的 provider 列表）
CREATE INDEX IF NOT EXISTS idx_user_oauth_links_user
  ON user_oauth_links (user_id, deleted_at);

CREATE INDEX IF NOT EXISTS idx_user_oauth_links_deleted_at
  ON user_oauth_links (deleted_at);
