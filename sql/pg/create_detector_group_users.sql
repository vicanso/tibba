CREATE TABLE detector_group_users (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  user_id BIGINT NOT NULL,
  group_id BIGINT NOT NULL,
  role SMALLINT NOT NULL DEFAULT 3,
  status SMALLINT NOT NULL DEFAULT 1,
  effective_start_time TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  effective_end_time TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  invited_by BIGINT DEFAULT NULL,
  remark VARCHAR(500) NOT NULL DEFAULT '',
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  created_by BIGINT NOT NULL,
  modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX uk_user_group ON detector_group_users (user_id, group_id, deleted_at);
CREATE INDEX idx_detector_group_users_deleted_at ON detector_group_users (deleted_at);
CREATE INDEX idx_effective_time ON detector_group_users (status, effective_start_time, effective_end_time, deleted_at);
CREATE INDEX idx_group_status ON detector_group_users (group_id, status);

CREATE TRIGGER set_detector_group_users_modified
  BEFORE UPDATE ON detector_group_users
  FOR EACH ROW EXECUTE FUNCTION trigger_set_modified_timestamp();

COMMENT ON TABLE detector_group_users IS '检测器分组用户关系表';
COMMENT ON COLUMN detector_group_users.id IS '主键ID';
COMMENT ON COLUMN detector_group_users.user_id IS '用户ID';
COMMENT ON COLUMN detector_group_users.group_id IS '组ID';
COMMENT ON COLUMN detector_group_users.role IS '用户在组中的角色：1-所有者，2-管理员，3-成员，4-查看者';
COMMENT ON COLUMN detector_group_users.status IS '状态，0：禁用，1：启用';
COMMENT ON COLUMN detector_group_users.effective_start_time IS '生效开始时间';
COMMENT ON COLUMN detector_group_users.effective_end_time IS '生效结束时间';
COMMENT ON COLUMN detector_group_users.invited_by IS '邀请人ID';
COMMENT ON COLUMN detector_group_users.remark IS '备注';
COMMENT ON COLUMN detector_group_users.created IS '创建时间';
COMMENT ON COLUMN detector_group_users.created_by IS '创建人';
COMMENT ON COLUMN detector_group_users.modified IS '更新时间';
COMMENT ON COLUMN detector_group_users.deleted_at IS '软删除时间';
