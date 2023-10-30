# tibba

TODO request review

## TODO

- [ ] swagger api生成
- [ ] 有无办法获取route的参数
- [ ] 有无办法获取所有路由
- [x] 支持jwt鉴权认证({id: 首次创建后不变, account: 账号, expired_at: 过期时间})

## 数据库及用户

```sql
CREATE DATABASE tibba CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

SHOW CREATE DATABASE tibba;

CREATE USER 'vicanso'@'%' IDENTIFIED BY 'A123456';
GRANT ALL ON tibba.* TO 'vicanso'@'%';
```