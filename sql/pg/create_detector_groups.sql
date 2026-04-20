CREATE TABLE detector_groups (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  name VARCHAR(255) NOT NULL,
  code VARCHAR(100) NOT NULL,
  owner_id BIGINT NOT NULL,
  status SMALLINT NOT NULL DEFAULT 1,
  remark VARCHAR(1000) NOT NULL DEFAULT '',
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  created_by BIGINT NOT NULL,
  modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX uk_code ON detector_groups (code, deleted_at);
CREATE INDEX idx_detector_groups_deleted_at ON detector_groups (deleted_at);
CREATE INDEX idx_owner_id ON detector_groups (owner_id);

CREATE TRIGGER set_detector_groups_modified
  BEFORE UPDATE ON detector_groups
  FOR EACH ROW EXECUTE FUNCTION trigger_set_modified_timestamp();

COMMENT ON TABLE detector_groups IS '检测器分组表';
COMMENT ON COLUMN detector_groups.id IS '主键ID';
COMMENT ON COLUMN detector_groups.name IS '组名称';
COMMENT ON COLUMN detector_groups.code IS '组代码，用于程序标识';
COMMENT ON COLUMN detector_groups.owner_id IS '组所有者ID';
COMMENT ON COLUMN detector_groups.status IS '状态，0：禁用，1：启用';
COMMENT ON COLUMN detector_groups.remark IS '备注';
COMMENT ON COLUMN detector_groups.created IS '创建时间';
COMMENT ON COLUMN detector_groups.created_by IS '创建人';
COMMENT ON COLUMN detector_groups.modified IS '更新时间';
COMMENT ON COLUMN detector_groups.deleted_at IS '软删除时间';
