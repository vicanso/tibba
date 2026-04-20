CREATE TABLE objects (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  "key" VARCHAR(2048) NOT NULL,
  value BYTEA,
  modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX idx_objects_key ON objects ("key");
CREATE INDEX idx_objects_modified ON objects (modified);
CREATE INDEX idx_objects_created ON objects (created);

CREATE TRIGGER set_objects_modified
  BEFORE UPDATE ON objects
  FOR EACH ROW EXECUTE FUNCTION trigger_set_modified_timestamp();

COMMENT ON TABLE objects IS '对象表';
COMMENT ON COLUMN objects.id IS '主键ID';
COMMENT ON COLUMN objects."key" IS '对象路径';
COMMENT ON COLUMN objects.value IS '对象内容';
COMMENT ON COLUMN objects.modified IS '修改时间';
COMMENT ON COLUMN objects.created IS '创建时间';
