-- 异步任务队列表（基于 PostgreSQL + FOR UPDATE SKIP LOCKED）。
--
-- 用途：把「慢、可失败、需重试、最好别阻塞请求」的活异步化——发邮件、出站
-- Webhook、生成导出、注册赠积分等。与 cron 调度器（tibba-scheduler）互补：cron
-- 管「周期性反复跑」，本表管「事件触发、可靠地做掉一次，失败能重试、毒消息隔离」。
--
-- 选 PG 而非 Redis 的关键理由：支持「事务性入队」（enqueue 与业务写库同事务），
-- 杜绝 dual-write 问题（业务提交了但任务没入队）。
--
-- 字段语义：
-- - queue        队列名，便于未来按队列/优先级分流（MVP worker 一并消费所有队列）
-- - job_type     任务类型，worker 据此路由到对应 handler
-- - payload      任务入参（JSONB）；建议只存引用（如 user_id）而非整对象快照
-- - status       0=待跑  1=执行中  3=死信（成功的行直接删除，不占空间）
-- - attempts     已尝试次数（认领时 +1）
-- - max_attempts 重试上限，超过即转死信
-- - run_at       下次可执行时间；延迟任务 / 重试退避都靠它推后
-- - locked_at    认领时间；配合可见性超时回收 worker 崩溃后卡住的行
-- - locked_by    认领的 worker 标识（仅排障用，互斥由行锁 + SKIP LOCKED 保证）
-- - last_error   最近一次失败信息（截断存储）
CREATE TABLE IF NOT EXISTS jobs (
  id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  queue        VARCHAR(64)  NOT NULL DEFAULT 'default',
  job_type     VARCHAR(128) NOT NULL,
  payload      JSONB        NOT NULL DEFAULT '{}'::jsonb,
  status       SMALLINT     NOT NULL DEFAULT 0,
  attempts     INT          NOT NULL DEFAULT 0,
  max_attempts INT          NOT NULL DEFAULT 25,
  run_at       TIMESTAMP    NOT NULL DEFAULT now(),
  locked_at    TIMESTAMP    DEFAULT NULL,
  locked_by    VARCHAR(64)  DEFAULT NULL,
  last_error   TEXT         DEFAULT NULL,
  created      TIMESTAMP    NOT NULL DEFAULT now()
);

-- 认领热点：只在「待跑」的行上按 run_at 建部分索引（status=0 且到点的最旧一条）
CREATE INDEX IF NOT EXISTS idx_jobs_claim ON jobs (run_at) WHERE status = 0;

-- 回收热点：只在「执行中」的行上按 locked_at 建部分索引（找超时未 ack 的行）
CREATE INDEX IF NOT EXISTS idx_jobs_reap ON jobs (locked_at) WHERE status = 1;
