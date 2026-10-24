-- 账号 ↔ 角色 的对应关系。
--
-- 原服务端这层由 `PlayerServerModule` 在内存里维护（key 是 openId+serverId），
-- 落库位置在官方数据库里没有随端打包，所以这里显式建模：
-- `open_id` 就是登录包里的 `LoginReq_4.openId`，`server_id` 用来区分分服。
--
-- 唯一性：(open_id, server_id) 一个账号在一个服只能有一个角色 ——
-- 对应原版创角第 14 步的 `DBPlayerModule.repeated(openId)` 语义。
ALTER TABLE player ADD COLUMN IF NOT EXISTS open_id VARCHAR(128);
ALTER TABLE player ADD COLUMN IF NOT EXISTS server_id INTEGER NOT NULL DEFAULT 0;
ALTER TABLE player ADD COLUMN IF NOT EXISTS create_time TIMESTAMPTZ NOT NULL DEFAULT now();

CREATE UNIQUE INDEX IF NOT EXISTS uq_player_open_server
    ON player (open_id, server_id)
    WHERE open_id IS NOT NULL;
