CREATE TABLE user_oauth_links (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  user_id BIGINT NOT NULL,
  provider VARCHAR(32) NOT NULL,
  provider_user_id VARCHAR(64) NOT NULL,
  provider_login VARCHAR(255) NOT NULL DEFAULT '',
  provider_email VARCHAR(255) NOT NULL DEFAULT '',
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX uk_user_oauth_links_provider_uid
  ON user_oauth_links (provider, provider_user_id, deleted_at);
CREATE INDEX idx_user_oauth_links_user ON user_oauth_links (user_id, deleted_at);
CREATE INDEX idx_user_oauth_links_deleted_at ON user_oauth_links (deleted_at);

COMMENT ON TABLE user_oauth_links IS '用户与第三方身份提供商的关联表';
COMMENT ON COLUMN user_oauth_links.id IS '主键ID';
COMMENT ON COLUMN user_oauth_links.user_id IS '本地 users.id';
COMMENT ON COLUMN user_oauth_links.provider IS '第三方 provider 名，如 "github"';
COMMENT ON COLUMN user_oauth_links.provider_user_id IS '第三方用户稳定 id（GitHub 数字 id / Google sub）';
COMMENT ON COLUMN user_oauth_links.provider_login IS '第三方 username 快照（展示用）';
COMMENT ON COLUMN user_oauth_links.provider_email IS '第三方 primary verified email 快照';
COMMENT ON COLUMN user_oauth_links.created IS '创建时间';
COMMENT ON COLUMN user_oauth_links.deleted_at IS '软删除时间（解绑后写入）';
