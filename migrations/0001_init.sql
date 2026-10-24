-- 0001：最小初始迁移。
--
-- 第 4 组还没有业务表（users/nodes 等在第 6、7 组），这里只创建 citext 扩展，
-- 用来验证「嵌入迁移 + 版本/校验和检查 + migrate/serve 子命令」的完整机制。
-- 生产环境由 RDS 管理员预先安装扩展也可以，`IF NOT EXISTS` 保证幂等。
CREATE EXTENSION IF NOT EXISTS citext;
