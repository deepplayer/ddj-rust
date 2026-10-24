-- 发号器。
--
-- 原版 `PlayerServerModule` 内存里发号，规则没有随端打包；
-- 这里用独立序列，保证「重启后不回头」，且与服号组合后可读：
--   player_id = server_id * 1_000_000 + nextval()
CREATE SEQUENCE IF NOT EXISTS player_id_seq START 1;
