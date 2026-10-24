-- 占位迁移：真正的 277 张表由 `data/sql/platform.sql` 导入，
-- 这里只保留 sqlx 迁移目录的形状，保证 `migrate!` 编译期能展开。
SELECT 1;
