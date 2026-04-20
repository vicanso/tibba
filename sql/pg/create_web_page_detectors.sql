CREATE TABLE web_page_detectors (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  status SMALLINT NOT NULL DEFAULT 0,
  name VARCHAR(255) NOT NULL,
  "interval" SMALLINT NOT NULL DEFAULT 1,
  url TEXT NOT NULL,
  width INTEGER NOT NULL DEFAULT 0,
  height INTEGER NOT NULL DEFAULT 0,
  user_agent VARCHAR(255) NOT NULL DEFAULT '',
  accept_language VARCHAR(255) NOT NULL DEFAULT '',
  platform VARCHAR(255) NOT NULL DEFAULT '',
  wait_for_element VARCHAR(255) NOT NULL DEFAULT '',
  device_scale_factor REAL NOT NULL DEFAULT 0,
  timeout INTEGER NOT NULL DEFAULT 0,
  capture_screenshot BOOLEAN NOT NULL DEFAULT FALSE,
  capture_element VARCHAR(255) NOT NULL DEFAULT '',
  remark VARCHAR(1000) NOT NULL DEFAULT '',
  regions JSONB NOT NULL DEFAULT '[]',
  created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  created_by BIGINT NOT NULL,
  modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  deleted_at TIMESTAMP DEFAULT NULL
);

CREATE UNIQUE INDEX web_page_detectors_name ON web_page_detectors (name, deleted_at);
CREATE INDEX idx_web_page_detectors_deleted_at ON web_page_detectors (deleted_at);

CREATE TRIGGER set_web_page_detectors_modified
  BEFORE UPDATE ON web_page_detectors
  FOR EACH ROW EXECUTE FUNCTION trigger_set_modified_timestamp();

COMMENT ON TABLE web_page_detectors IS '网页检测器表';
COMMENT ON COLUMN web_page_detectors.id IS '主键ID';
COMMENT ON COLUMN web_page_detectors.status IS '状态，0：禁用，1：启用';
COMMENT ON COLUMN web_page_detectors.name IS '名称';
COMMENT ON COLUMN web_page_detectors."interval" IS '间隔时间，单位：分钟';
COMMENT ON COLUMN web_page_detectors.url IS 'URL';
COMMENT ON COLUMN web_page_detectors.width IS '宽度';
COMMENT ON COLUMN web_page_detectors.height IS '高度';
COMMENT ON COLUMN web_page_detectors.user_agent IS '用户代理';
COMMENT ON COLUMN web_page_detectors.accept_language IS '接受语言';
COMMENT ON COLUMN web_page_detectors.platform IS '平台';
COMMENT ON COLUMN web_page_detectors.wait_for_element IS '等待元素';
COMMENT ON COLUMN web_page_detectors.device_scale_factor IS '设备缩放因子';
COMMENT ON COLUMN web_page_detectors.timeout IS '超时时间，单位：秒';
COMMENT ON COLUMN web_page_detectors.capture_screenshot IS '是否捕获截图';
COMMENT ON COLUMN web_page_detectors.capture_element IS '捕获元素';
COMMENT ON COLUMN web_page_detectors.remark IS '备注';
COMMENT ON COLUMN web_page_detectors.regions IS '触发区域';
COMMENT ON COLUMN web_page_detectors.created IS '创建时间';
COMMENT ON COLUMN web_page_detectors.created_by IS '创建人';
COMMENT ON COLUMN web_page_detectors.modified IS '更新时间';
COMMENT ON COLUMN web_page_detectors.deleted_at IS '软删除时间';
