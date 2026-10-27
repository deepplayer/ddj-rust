-- 角色渠道字段：`channel_id` / `cid`（原版 `Player.channelId` / `Player.cid`）。
--
-- 原版由 `PlayerCached.createPlayer(..., int channelId, String cid, ...)`
-- 从 `CreatePlayerReq_6.channelId` / `CreatePlayerReq_6.cid` 写入
-- （`src/.../CreatePlayerReq_6` 的字段 11 / 12）。
--
-- 这两个字段是大量活动的准入门槛，例如：
--   - `PlatformServerModule.isEligibleShortcut(channelId, cid)` → 17301/17302/17303
--     （保存桌面：`bf_config.configId=59` 按 channelId 配 cid 白名单）
--   - `PlatformServerModule.FAVOURABLE_CHANNELS.contains(channelId)` → 17101/17102/17103
--     （渠道好评：`bf_config.configId=21` 配渠道白名单）
--   - `ChannelDrawDBCache.getByCid(cid)` → 17081（渠道卡池）
--   - `PrayCidMessageHandler` 的 `cidTimeMap.get(cid)` → 17081 的时间窗
--
-- 两者都可为空：原版 `Player` 构造里没做非空校验，老角色可能只有其中一个。
ALTER TABLE player ADD COLUMN IF NOT EXISTS channel_id INTEGER NOT NULL DEFAULT 0;
ALTER TABLE player ADD COLUMN IF NOT EXISTS cid VARCHAR(64) NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS idx_player_channel ON player (channel_id);
