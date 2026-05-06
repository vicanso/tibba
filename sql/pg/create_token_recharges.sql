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
