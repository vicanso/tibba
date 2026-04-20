CREATE TABLE files (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  filename VARCHAR(255) NOT NULL,
  file_size BIGINT NOT NULL,
  content_type VARCHAR(100) NOT NULL,
  "group" VARCHAR(100) NOT NULL,
  uploader VARCHAR(100) NOT NULL,
  image_width INTEGER NOT NULL DEFAULT -1,
  image_height INTEGER NOT NULL DEFAULT -1,
  metadata JSONB NOT NULL DEFAULT '{}',
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX file_name ON files (filename, deleted_at);
CREATE INDEX idx_files_deleted_at ON files (deleted_at);

CREATE TRIGGER set_files_modified
  BEFORE UPDATE ON files
  FOR EACH ROW EXECUTE FUNCTION trigger_set_modified_timestamp();

COMMENT ON TABLE files IS '文件表';
COMMENT ON COLUMN files.id IS '主键ID';
COMMENT ON COLUMN files.filename IS '文件名';
COMMENT ON COLUMN files.file_size IS '文件大小';
COMMENT ON COLUMN files.content_type IS '内容类型';
COMMENT ON COLUMN files."group" IS '分组';
COMMENT ON COLUMN files.uploader IS '上传者';
COMMENT ON COLUMN files.image_width IS '图片宽度';
COMMENT ON COLUMN files.image_height IS '图片高度';
COMMENT ON COLUMN files.metadata IS '存储其他元数据信息';
COMMENT ON COLUMN files.created IS '创建时间';
COMMENT ON COLUMN files.modified IS '更新时间';
COMMENT ON COLUMN files.deleted_at IS '软删除时间';
