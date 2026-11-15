-- Standard 层安全与性能加固迁移：
-- 为 http_stats(created) 建立仅覆盖未删除行的局部索引，避免历史探测记录范围查询退化为全表扫描。

CREATE INDEX IF NOT EXISTS idx_http_stats_created
    ON http_stats (created)
    WHERE deleted_at IS NULL;
