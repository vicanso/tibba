-- 种子数据：登记通配权限码 "*"，并把它授予 "su"（超级管理员）角色。
-- 已存在则跳过，保证迁移幂等。
-- 与 sql/pg/create_role_permissions.sql 末尾的种子 INSERT 等价。
INSERT INTO permissions (code, description)
VALUES ('*', 'Wildcard — grants every action')
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role, permission_code)
VALUES ('su', '*')
ON CONFLICT DO NOTHING;
