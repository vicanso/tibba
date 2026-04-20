CREATE TABLE configurations (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  status SMALLINT NOT NULL DEFAULT 0,
  category VARCHAR(50) NOT NULL,
  name VARCHAR(100) NOT NULL,
  data JSONB NOT NULL,
  description VARCHAR(255) NOT NULL DEFAULT '',
  effective_start_time TIMESTAMP NOT NULL,
  effective_end_time TIMESTAMP NOT NULL,
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX uk_category_name ON configurations (category, name, deleted_at);
CREATE INDEX idx_configurations_effective_time ON configurations (status, effective_start_time, effective_end_time, deleted_at);

CREATE TRIGGER set_configurations_modified
  BEFORE UPDATE ON configurations
  FOR EACH ROW EXECUTE FUNCTION trigger_set_modified_timestamp();

COMMENT ON TABLE configurations IS '系统配置表';
COMMENT ON COLUMN configurations.id IS '主键ID';
COMMENT ON COLUMN configurations.status IS '状态，0：禁用，1：启用';
COMMENT ON COLUMN configurations.category IS '配置类型';
COMMENT ON COLUMN configurations.name IS '配置名称';
COMMENT ON COLUMN configurations.data IS '配置数据';
COMMENT ON COLUMN configurations.description IS '配置描述';
COMMENT ON COLUMN configurations.effective_start_time IS '生效开始时间';
COMMENT ON COLUMN configurations.effective_end_time IS '生效结束时间';
COMMENT ON COLUMN configurations.created IS '创建时间';
COMMENT ON COLUMN configurations.modified IS '更新时间';
COMMENT ON COLUMN configurations.deleted_at IS '软删除时间';
