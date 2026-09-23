-- 基线迁移：补齐此前只存在于 sql/pg/（「参考 / 历史」目录）的 16 张表。
--
-- 问题：README 声明 schema 由 sqlx::migrate! 自动应用、以 migrations/ 为真相来源，
-- 但 users / configurations / files / token_* 等表从未进入 migrations/。按 README
-- 从空库启动时，20260606000004 的 `ALTER TABLE users` 直接失败，应用起不来。
--
-- 版本号刻意早于所有既有迁移：sqlx 对「未应用」的迁移不检查版本先后，因此
-- - 全新库：本迁移最先执行，建好表，后续 ALTER / 外键才有对象可引用；
-- - 既有库（表早已按 sql/pg 手工建好）：每张表都包在「不存在才建」的 DO 块里，
--   整块跳过——连索引与注释都不碰，避免老库上某张表缺列时 COMMENT / CREATE INDEX
--   报错，把本来能启动的实例拖垮。
--
-- 列定义取自 sql/pg/create_*.sql，已与代码中 INSERT / UPDATE / FromRow 用到的列
-- 逐一核对。之后的 schema 变更请照常新增迁移，不要修改本文件。

-- users
DO $baseline$
BEGIN
  IF to_regclass('users') IS NULL THEN
    CREATE TABLE users (
      id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
      status SMALLINT NOT NULL DEFAULT 0,
      account VARCHAR(255) NOT NULL,
      password VARCHAR(255) NOT NULL,
      roles JSONB NOT NULL DEFAULT '[]',
      "groups" JSONB NOT NULL DEFAULT '[]',
      nickname VARCHAR(100) DEFAULT NULL,
      phone VARCHAR(64) DEFAULT NULL,
      remark VARCHAR(255) NOT NULL DEFAULT '',
      email VARCHAR(255) NOT NULL DEFAULT '',
      avatar VARCHAR(1024) NOT NULL DEFAULT '',
      last_login_at TIMESTAMP DEFAULT NULL,
      email_verified_at TIMESTAMP DEFAULT NULL,
      totp_secret TEXT DEFAULT NULL,
      totp_enabled_at TIMESTAMP DEFAULT NULL,
      totp_recovery_codes JSONB DEFAULT NULL,
      created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
      modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
      deleted_at TIMESTAMP DEFAULT NULL
    );

    CREATE UNIQUE INDEX user_account ON users (account) WHERE deleted_at IS NULL;
    CREATE INDEX idx_users_deleted_at ON users (deleted_at);

    COMMENT ON TABLE users IS '用户表';
    COMMENT ON COLUMN users.id IS '主键ID';
    COMMENT ON COLUMN users.status IS '状态，0：禁用，1：启用';
    COMMENT ON COLUMN users.password IS '密码';
    COMMENT ON COLUMN users.roles IS '用户角色';
    COMMENT ON COLUMN users."groups" IS '用户群组';
    COMMENT ON COLUMN users.nickname IS '用户昵称';
    COMMENT ON COLUMN users.phone IS '手机号';
    COMMENT ON COLUMN users.last_login_at IS '最后登录时间';
    COMMENT ON COLUMN users.email_verified_at IS '邮箱验证通过时间，NULL 表示未验证';
    COMMENT ON COLUMN users.totp_secret IS 'TOTP 密钥，AES-256-GCM 加密后 base64；NULL 表示未注册 2FA';
    COMMENT ON COLUMN users.totp_enabled_at IS '2FA 激活时间；NULL 表示未启用';
    COMMENT ON COLUMN users.totp_recovery_codes IS '一次性恢复码的 SHA-256(base64) 数组';
    COMMENT ON COLUMN users.remark IS '备注';
    COMMENT ON COLUMN users.email IS '用户邮箱';
    COMMENT ON COLUMN users.avatar IS '用户头像';
    COMMENT ON COLUMN users.created IS '创建时间';
    COMMENT ON COLUMN users.modified IS '更新时间';
    COMMENT ON COLUMN users.deleted_at IS '软删除时间';
  END IF;
END
$baseline$;

-- configurations
DO $baseline$
BEGIN
  IF to_regclass('configurations') IS NULL THEN
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

    CREATE UNIQUE INDEX uk_category_name ON configurations (category, name) WHERE deleted_at IS NULL;
    CREATE INDEX idx_configurations_effective_time ON configurations (status, effective_start_time, effective_end_time, deleted_at);

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
  END IF;
END
$baseline$;

-- files
DO $baseline$
BEGIN
  IF to_regclass('files') IS NULL THEN
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

    CREATE UNIQUE INDEX file_name ON files (filename) WHERE deleted_at IS NULL;
    CREATE INDEX idx_files_deleted_at ON files (deleted_at);

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
  END IF;
END
$baseline$;

-- objects
DO $baseline$
BEGIN
  IF to_regclass('objects') IS NULL THEN
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

    COMMENT ON TABLE objects IS '对象表';
    COMMENT ON COLUMN objects.id IS '主键ID';
    COMMENT ON COLUMN objects."key" IS '对象路径';
    COMMENT ON COLUMN objects.value IS '对象内容';
    COMMENT ON COLUMN objects.modified IS '修改时间';
    COMMENT ON COLUMN objects.created IS '创建时间';
  END IF;
END
$baseline$;

-- http_detectors
DO $baseline$
BEGIN
  IF to_regclass('http_detectors') IS NULL THEN
    CREATE TABLE http_detectors (
      id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
      status SMALLINT NOT NULL DEFAULT 0,
      name VARCHAR(255) NOT NULL,
      "interval" SMALLINT NOT NULL DEFAULT 1,
      url TEXT NOT NULL,
      method VARCHAR(10) NOT NULL DEFAULT 'GET',
      alpn_protocols JSONB NOT NULL DEFAULT '[]',
      resolves JSONB NOT NULL DEFAULT '[]',
      headers JSONB NOT NULL DEFAULT '{}',
      ip_version SMALLINT NOT NULL DEFAULT 0,
      skip_verify BOOLEAN NOT NULL DEFAULT FALSE,
      retries SMALLINT NOT NULL DEFAULT 0,
      failure_threshold SMALLINT NOT NULL DEFAULT 0,
      dns_servers JSONB NOT NULL DEFAULT '[]',
      body BYTEA,
      script TEXT,
      alarm_url VARCHAR(1024) NOT NULL DEFAULT '',
      random_querystring BOOLEAN NOT NULL DEFAULT FALSE,
      alarm_on_change BOOLEAN NOT NULL DEFAULT FALSE,
      "verbose" BOOLEAN NOT NULL DEFAULT FALSE,
      regions JSONB NOT NULL DEFAULT '[]',
      group_id BIGINT NOT NULL,
      created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
      created_by BIGINT NOT NULL,
      remark VARCHAR(1000) NOT NULL DEFAULT '',
      modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
      deleted_at TIMESTAMP DEFAULT NULL
    );

    CREATE UNIQUE INDEX name_group_id ON http_detectors (name, group_id) WHERE deleted_at IS NULL;
    CREATE INDEX idx_http_detectors_deleted_at ON http_detectors (deleted_at);

    COMMENT ON TABLE http_detectors IS 'HTTP检测器表';
    COMMENT ON COLUMN http_detectors.id IS '主键ID';
    COMMENT ON COLUMN http_detectors.status IS '状态，0：禁用，1：启用';
    COMMENT ON COLUMN http_detectors.name IS '名称';
    COMMENT ON COLUMN http_detectors."interval" IS '间隔时间，单位：分钟';
    COMMENT ON COLUMN http_detectors.url IS 'URL';
    COMMENT ON COLUMN http_detectors.method IS 'HTTP方法';
    COMMENT ON COLUMN http_detectors.alpn_protocols IS 'ALPN协议';
    COMMENT ON COLUMN http_detectors.resolves IS 'DNS解析';
    COMMENT ON COLUMN http_detectors.headers IS 'HTTP头';
    COMMENT ON COLUMN http_detectors.ip_version IS 'IP版本';
    COMMENT ON COLUMN http_detectors.skip_verify IS '是否跳过证书验证';
    COMMENT ON COLUMN http_detectors.retries IS '重试次数';
    COMMENT ON COLUMN http_detectors.failure_threshold IS '失败阈值';
    COMMENT ON COLUMN http_detectors.dns_servers IS 'DNS服务器';
    COMMENT ON COLUMN http_detectors.body IS '请求体';
    COMMENT ON COLUMN http_detectors.script IS '脚本';
    COMMENT ON COLUMN http_detectors.alarm_url IS '告警URL';
    COMMENT ON COLUMN http_detectors.random_querystring IS '是否添加随机查询字符串';
    COMMENT ON COLUMN http_detectors.alarm_on_change IS '是否仅在状态变更时推送告警';
    COMMENT ON COLUMN http_detectors."verbose" IS '是否详细输出';
    COMMENT ON COLUMN http_detectors.regions IS '触发区域';
    COMMENT ON COLUMN http_detectors.group_id IS '组ID';
    COMMENT ON COLUMN http_detectors.created IS '创建时间';
    COMMENT ON COLUMN http_detectors.created_by IS '创建人';
    COMMENT ON COLUMN http_detectors.remark IS '备注';
    COMMENT ON COLUMN http_detectors.modified IS '更新时间';
    COMMENT ON COLUMN http_detectors.deleted_at IS '软删除时间';
  END IF;
END
$baseline$;

-- http_stats
DO $baseline$
BEGIN
  IF to_regclass('http_stats') IS NULL THEN
    CREATE TABLE http_stats (
      id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
      target_id BIGINT NOT NULL,
      target_name VARCHAR(255) NOT NULL DEFAULT '',
      url TEXT NOT NULL,
      dns_lookup INTEGER NOT NULL DEFAULT -1,
      quic_connect INTEGER NOT NULL DEFAULT -1,
      tcp_connect INTEGER NOT NULL DEFAULT -1,
      tls_handshake INTEGER NOT NULL DEFAULT -1,
      server_processing INTEGER NOT NULL DEFAULT -1,
      content_transfer INTEGER NOT NULL DEFAULT -1,
      total INTEGER NOT NULL DEFAULT -1,
      addr VARCHAR(255) NOT NULL DEFAULT '',
      status_code SMALLINT NOT NULL DEFAULT 0,
      tls VARCHAR(20) NOT NULL DEFAULT '',
      alpn VARCHAR(10) NOT NULL DEFAULT '',
      subject VARCHAR(1000) NOT NULL DEFAULT '',
      issuer VARCHAR(1000) NOT NULL DEFAULT '',
      cert_not_before VARCHAR(32) NOT NULL DEFAULT '',
      cert_not_after VARCHAR(32) NOT NULL DEFAULT '',
      cert_cipher VARCHAR(50) NOT NULL DEFAULT '',
      cert_domains VARCHAR(3000) NOT NULL DEFAULT '',
      body_size INTEGER NOT NULL DEFAULT -1,
      region VARCHAR(64) NOT NULL DEFAULT '',
      error TEXT,
      result SMALLINT NOT NULL DEFAULT 0,
      remark VARCHAR(1000) NOT NULL DEFAULT '',
      created TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
      modified TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
      deleted_at TIMESTAMP DEFAULT NULL
    );

    CREATE INDEX idx_http_stats_deleted_at ON http_stats (deleted_at);
    CREATE INDEX idx_target_id_result ON http_stats (target_id, result);
    CREATE INDEX idx_http_stats_modified ON http_stats (modified);

    COMMENT ON TABLE http_stats IS 'HTTP统计表';
    COMMENT ON COLUMN http_stats.id IS '主键ID';
    COMMENT ON COLUMN http_stats.target_id IS '目标ID';
    COMMENT ON COLUMN http_stats.target_name IS '目标名称';
    COMMENT ON COLUMN http_stats.url IS 'URL';
    COMMENT ON COLUMN http_stats.dns_lookup IS 'DNS查询时间';
    COMMENT ON COLUMN http_stats.quic_connect IS 'QUIC连接时间';
    COMMENT ON COLUMN http_stats.tcp_connect IS 'TCP连接时间';
    COMMENT ON COLUMN http_stats.tls_handshake IS 'TLS握手时间';
    COMMENT ON COLUMN http_stats.server_processing IS '服务器处理时间';
    COMMENT ON COLUMN http_stats.content_transfer IS '内容传输时间';
    COMMENT ON COLUMN http_stats.total IS '总时间';
    COMMENT ON COLUMN http_stats.addr IS '地址';
    COMMENT ON COLUMN http_stats.status_code IS '状态码';
    COMMENT ON COLUMN http_stats.tls IS 'TLS版本';
    COMMENT ON COLUMN http_stats.alpn IS 'ALPN';
    COMMENT ON COLUMN http_stats.subject IS '证书主题';
    COMMENT ON COLUMN http_stats.issuer IS '证书颁发者';
    COMMENT ON COLUMN http_stats.cert_not_before IS '证书有效期开始时间';
    COMMENT ON COLUMN http_stats.cert_not_after IS '证书有效期结束时间';
    COMMENT ON COLUMN http_stats.cert_cipher IS '证书加密套件';
    COMMENT ON COLUMN http_stats.cert_domains IS '证书域名';
    COMMENT ON COLUMN http_stats.body_size IS '响应体大小';
    COMMENT ON COLUMN http_stats.region IS '触发区域';
    COMMENT ON COLUMN http_stats.error IS '错误信息';
    COMMENT ON COLUMN http_stats.result IS '结果';
    COMMENT ON COLUMN http_stats.remark IS '备注';
    COMMENT ON COLUMN http_stats.created IS '创建时间';
    COMMENT ON COLUMN http_stats.modified IS '更新时间';
    COMMENT ON COLUMN http_stats.deleted_at IS '软删除时间';
  END IF;
END
$baseline$;

-- web_page_detectors
DO $baseline$
BEGIN
  IF to_regclass('web_page_detectors') IS NULL THEN
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

    CREATE UNIQUE INDEX web_page_detectors_name ON web_page_detectors (name) WHERE deleted_at IS NULL;
    CREATE INDEX idx_web_page_detectors_deleted_at ON web_page_detectors (deleted_at);

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
  END IF;
END
$baseline$;

-- detector_groups
DO $baseline$
BEGIN
  IF to_regclass('detector_groups') IS NULL THEN
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

    CREATE UNIQUE INDEX uk_code ON detector_groups (code) WHERE deleted_at IS NULL;
    CREATE INDEX idx_detector_groups_deleted_at ON detector_groups (deleted_at);
    CREATE INDEX idx_owner_id ON detector_groups (owner_id);

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
  END IF;
END
$baseline$;

-- detector_group_users
DO $baseline$
BEGIN
  IF to_regclass('detector_group_users') IS NULL THEN
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

    CREATE UNIQUE INDEX uk_user_group ON detector_group_users (user_id, group_id) WHERE deleted_at IS NULL;
    CREATE INDEX idx_detector_group_users_deleted_at ON detector_group_users (deleted_at);
    CREATE INDEX idx_effective_time ON detector_group_users (status, effective_start_time, effective_end_time, deleted_at);
    CREATE INDEX idx_group_status ON detector_group_users (group_id, status);

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
  END IF;
END
$baseline$;

-- token_accounts
DO $baseline$
BEGIN
  IF to_regclass('token_accounts') IS NULL THEN
    CREATE TABLE token_accounts (
      id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
      user_id        BIGINT    NOT NULL,
      balance        BIGINT    NOT NULL DEFAULT 0,
      total_recharged BIGINT   NOT NULL DEFAULT 0,
      total_consumed  BIGINT   NOT NULL DEFAULT 0,
      status         SMALLINT  NOT NULL DEFAULT 1,
      remark         VARCHAR(500) NOT NULL DEFAULT '',
      created        TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
      modified       TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
      deleted_at     TIMESTAMP DEFAULT NULL
    );

    CREATE UNIQUE INDEX uk_token_accounts_user ON token_accounts (user_id) WHERE deleted_at IS NULL;
    CREATE INDEX idx_token_accounts_deleted_at ON token_accounts (deleted_at);

    COMMENT ON TABLE token_accounts IS '积分账户表';
    COMMENT ON COLUMN token_accounts.id IS '主键ID';
    COMMENT ON COLUMN token_accounts.user_id IS '用户ID';
    COMMENT ON COLUMN token_accounts.balance IS '当前可用积分';
    COMMENT ON COLUMN token_accounts.total_recharged IS '历史累计充值积分';
    COMMENT ON COLUMN token_accounts.total_consumed IS '历史累计消费积分';
    COMMENT ON COLUMN token_accounts.status IS '账户状态，1：正常，0：冻结';
    COMMENT ON COLUMN token_accounts.remark IS '备注';
    COMMENT ON COLUMN token_accounts.created IS '创建时间';
    COMMENT ON COLUMN token_accounts.modified IS '更新时间';
    COMMENT ON COLUMN token_accounts.deleted_at IS '软删除时间';
  END IF;
END
$baseline$;

-- token_recharges
DO $baseline$
BEGIN
  IF to_regclass('token_recharges') IS NULL THEN
    CREATE TABLE token_recharges (
      id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
      user_id    BIGINT       NOT NULL,
      amount     BIGINT       NOT NULL,
      source     SMALLINT     NOT NULL DEFAULT 1,
      order_id   VARCHAR(64)  NOT NULL DEFAULT '',
      expired_at TIMESTAMP    DEFAULT NULL,
      remark     VARCHAR(500) NOT NULL DEFAULT '',
      created_by BIGINT       NOT NULL DEFAULT 0,
      created    TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
      modified   TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
      deleted_at TIMESTAMP    DEFAULT NULL
    );

    CREATE INDEX idx_token_recharges_user ON token_recharges (user_id, created);
    CREATE INDEX idx_token_recharges_order ON token_recharges (order_id) WHERE order_id <> '';

    COMMENT ON TABLE token_recharges IS '积分充值记录表';
    COMMENT ON COLUMN token_recharges.id IS '主键ID';
    COMMENT ON COLUMN token_recharges.user_id IS '用户ID';
    COMMENT ON COLUMN token_recharges.amount IS '本次充值积分数';
    COMMENT ON COLUMN token_recharges.source IS '充值来源：1购买 2赠送 3退款 4管理员调整';
    COMMENT ON COLUMN token_recharges.order_id IS '关联支付订单号';
    COMMENT ON COLUMN token_recharges.expired_at IS '积分有效期，NULL表示永不过期';
    COMMENT ON COLUMN token_recharges.remark IS '备注';
    COMMENT ON COLUMN token_recharges.created_by IS '操作人ID（管理员调整时记录）';
    COMMENT ON COLUMN token_recharges.created IS '创建时间';
    COMMENT ON COLUMN token_recharges.modified IS '更新时间';
    COMMENT ON COLUMN token_recharges.deleted_at IS '软删除时间';
  END IF;
END
$baseline$;

-- token_usages
DO $baseline$
BEGIN
  IF to_regclass('token_usages') IS NULL THEN
    CREATE TABLE token_usages (
      id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
      user_id       BIGINT       NOT NULL,
      service       VARCHAR(64)  NOT NULL,
      amount        BIGINT       NOT NULL,
      model         VARCHAR(128) NOT NULL DEFAULT '',
      input_tokens  INT          NOT NULL DEFAULT 0,
      output_tokens INT          NOT NULL DEFAULT 0,
      api_path      VARCHAR(256) NOT NULL DEFAULT '',
      duration_ms   INT          NOT NULL DEFAULT 0,
      biz_id        VARCHAR(128) NOT NULL DEFAULT '',
      remark        VARCHAR(500) NOT NULL DEFAULT '',
      created       TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
      modified      TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
      deleted_at    TIMESTAMP    DEFAULT NULL
    );

    CREATE INDEX idx_token_usages_user    ON token_usages (user_id, created);
    CREATE INDEX idx_token_usages_service ON token_usages (service, model, created);
    CREATE INDEX idx_token_usages_biz     ON token_usages (biz_id) WHERE biz_id <> '';

    COMMENT ON TABLE token_usages IS '积分消耗记录表';
    COMMENT ON COLUMN token_usages.id IS '主键ID';
    COMMENT ON COLUMN token_usages.user_id IS '用户ID';
    COMMENT ON COLUMN token_usages.service IS '服务类型：llm、api、storage等';
    COMMENT ON COLUMN token_usages.amount IS '本次扣除积分数';
    COMMENT ON COLUMN token_usages.model IS 'LLM模型名称，非LLM场景为空';
    COMMENT ON COLUMN token_usages.input_tokens IS '输入token数，非LLM场景为0';
    COMMENT ON COLUMN token_usages.output_tokens IS '输出token数，非LLM场景为0';
    COMMENT ON COLUMN token_usages.api_path IS 'API路径，通用API场景使用';
    COMMENT ON COLUMN token_usages.duration_ms IS '调用耗时（毫秒）';
    COMMENT ON COLUMN token_usages.biz_id IS '关联业务ID（请求ID、任务ID等）';
    COMMENT ON COLUMN token_usages.remark IS '备注';
    COMMENT ON COLUMN token_usages.created IS '创建时间';
    COMMENT ON COLUMN token_usages.modified IS '更新时间';
    COMMENT ON COLUMN token_usages.deleted_at IS '软删除时间';
  END IF;
END
$baseline$;

-- token_keys
DO $baseline$
BEGIN
  IF to_regclass('token_keys') IS NULL THEN
    CREATE TABLE token_keys (
      id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
      user_id     BIGINT        NOT NULL,
      token       VARCHAR(64)   NOT NULL,
      name        VARCHAR(100)  NOT NULL DEFAULT '',
      status      SMALLINT      NOT NULL DEFAULT 1,
      expired_at  TIMESTAMP     DEFAULT NULL,
      created_by  BIGINT        NOT NULL DEFAULT 0,
      created     TIMESTAMP     NOT NULL DEFAULT CURRENT_TIMESTAMP,
      modified    TIMESTAMP     NOT NULL DEFAULT CURRENT_TIMESTAMP,
      deleted_at  TIMESTAMP     DEFAULT NULL
    );

    CREATE UNIQUE INDEX uk_token_keys_token ON token_keys (token) WHERE deleted_at IS NULL;
    CREATE INDEX idx_token_keys_user_id ON token_keys (user_id);
    CREATE INDEX idx_token_keys_deleted_at ON token_keys (deleted_at);

    COMMENT ON TABLE token_keys IS 'API 鉴权密钥表';
    COMMENT ON COLUMN token_keys.id IS '主键ID';
    COMMENT ON COLUMN token_keys.user_id IS '关联用户ID';
    COMMENT ON COLUMN token_keys.token IS 'API 密钥（UUID v4）';
    COMMENT ON COLUMN token_keys.name IS '密钥备注名称';
    COMMENT ON COLUMN token_keys.status IS '状态，1：启用，0：禁用';
    COMMENT ON COLUMN token_keys.expired_at IS '过期时间，NULL 表示永不过期';
    COMMENT ON COLUMN token_keys.created_by IS '创建人用户ID';
    COMMENT ON COLUMN token_keys.created IS '创建时间';
    COMMENT ON COLUMN token_keys.modified IS '更新时间';
    COMMENT ON COLUMN token_keys.deleted_at IS '软删除时间';
  END IF;
END
$baseline$;

-- token_prices
DO $baseline$
BEGIN
  IF to_regclass('token_prices') IS NULL THEN
    CREATE TABLE token_prices (
      id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
      service      VARCHAR(64)  NOT NULL,
      model        VARCHAR(128) NOT NULL DEFAULT '',
      input_price  BIGINT       NOT NULL DEFAULT 0,
      output_price BIGINT       NOT NULL DEFAULT 0,
      fixed_price  BIGINT       NOT NULL DEFAULT 0,
      unit_size    INT          NOT NULL DEFAULT 1000,
      status       SMALLINT     NOT NULL DEFAULT 1,
      remark       VARCHAR(500) NOT NULL DEFAULT '',
      created      TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
      modified     TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
      deleted_at   TIMESTAMP    DEFAULT NULL
    );

    CREATE UNIQUE INDEX uk_token_prices_service_model ON token_prices (service, model) WHERE deleted_at IS NULL;

    COMMENT ON TABLE token_prices IS '积分定价配置表';
    COMMENT ON COLUMN token_prices.id IS '主键ID';
    COMMENT ON COLUMN token_prices.service IS '服务类型：llm、api等';
    COMMENT ON COLUMN token_prices.model IS '模型名称，通用API场景为空字符串';
    COMMENT ON COLUMN token_prices.input_price IS '每unit_size个输入token扣除的积分数';
    COMMENT ON COLUMN token_prices.output_price IS '每unit_size个输出token扣除的积分数';
    COMMENT ON COLUMN token_prices.fixed_price IS '每次调用固定扣除积分数';
    COMMENT ON COLUMN token_prices.unit_size IS '计费基数，默认1000（即per 1K tokens）';
    COMMENT ON COLUMN token_prices.status IS '状态，1：启用，0：禁用';
    COMMENT ON COLUMN token_prices.remark IS '备注';
    COMMENT ON COLUMN token_prices.created IS '创建时间';
    COMMENT ON COLUMN token_prices.modified IS '更新时间';
    COMMENT ON COLUMN token_prices.deleted_at IS '软删除时间';
  END IF;
END
$baseline$;

-- token_llms
DO $baseline$
BEGIN
  IF to_regclass('token_llms') IS NULL THEN
    CREATE TABLE token_llms (
      id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
      name       VARCHAR(100) NOT NULL,
      url        VARCHAR(500) NOT NULL,
      model      VARCHAR(128) NOT NULL,
      api_key    VARCHAR(500) NOT NULL,
      provider   VARCHAR(20)  NOT NULL DEFAULT 'openai',
      status     SMALLINT     NOT NULL DEFAULT 1,
      remark     VARCHAR(500) NOT NULL DEFAULT '',
      created    TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
      modified   TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
      deleted_at TIMESTAMP    DEFAULT NULL
    );

    CREATE UNIQUE INDEX uk_token_llms_name ON token_llms (name) WHERE deleted_at IS NULL;

    COMMENT ON TABLE token_llms IS 'LLM 服务配置表';
    COMMENT ON COLUMN token_llms.id      IS '主键ID';
    COMMENT ON COLUMN token_llms.name    IS '配置名称（唯一），如 default、premium 等';
    COMMENT ON COLUMN token_llms.url     IS 'LLM API base URL';
    COMMENT ON COLUMN token_llms.model   IS '模型名（与 token_prices.model 对应用于计费）';
    COMMENT ON COLUMN token_llms.api_key IS 'LLM API 密钥';
    COMMENT ON COLUMN token_llms.provider IS '后端协议：openai（默认）或 anthropic';
    COMMENT ON COLUMN token_llms.status  IS '状态，1：启用，0：禁用';
    COMMENT ON COLUMN token_llms.remark  IS '备注';
    COMMENT ON COLUMN token_llms.created IS '创建时间';
    COMMENT ON COLUMN token_llms.modified IS '更新时间';
    COMMENT ON COLUMN token_llms.deleted_at IS '软删除时间';
  END IF;
END
$baseline$;

-- docker_analyses
DO $baseline$
BEGIN
  IF to_regclass('docker_analyses') IS NULL THEN
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
  END IF;
END
$baseline$;
