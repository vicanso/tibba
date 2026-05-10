CREATE TABLE docker_analyses (
  id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  user_id     BIGINT       NOT NULL,
  repo_name   VARCHAR(500) NOT NULL,
  tag         VARCHAR(200) NOT NULL DEFAULT '',
  status      SMALLINT     NOT NULL DEFAULT 0,
  result      TEXT,
  notify_type VARCHAR(20)  NOT NULL DEFAULT '',
  notify_data VARCHAR(500) NOT NULL DEFAULT '',
  notify_force BOOLEAN     NOT NULL DEFAULT FALSE,
  created     TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
  modified    TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_docker_analyses_user_repo ON docker_analyses (user_id, repo_name);
CREATE INDEX idx_docker_analyses_status    ON docker_analyses (status);
CREATE INDEX idx_docker_analyses_created   ON docker_analyses (created);

COMMENT ON TABLE docker_analyses IS 'Docker 镜像分析任务表';
COMMENT ON COLUMN docker_analyses.id          IS '主键ID';
COMMENT ON COLUMN docker_analyses.user_id     IS '发起分析的用户ID';
COMMENT ON COLUMN docker_analyses.repo_name   IS 'Docker 仓库名（namespace/name）';
COMMENT ON COLUMN docker_analyses.tag         IS '镜像标签';
COMMENT ON COLUMN docker_analyses.status      IS '任务状态：0=等待处理，1=处理中，2=已完成，3=失败';
COMMENT ON COLUMN docker_analyses.result      IS '分析结果（JSON 字符串）';
COMMENT ON COLUMN docker_analyses.notify_type IS '推送方式：wecom / email / 空字符串表示无推送';
COMMENT ON COLUMN docker_analyses.notify_data IS '推送目标：WeCom robot key 或收件邮箱地址';
COMMENT ON COLUMN docker_analyses.notify_force IS '是否在结果与上次一致时仍发送通知';
COMMENT ON COLUMN docker_analyses.created     IS '创建时间';
COMMENT ON COLUMN docker_analyses.modified    IS '更新时间';
