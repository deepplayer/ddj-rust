-- 玩家银两 / 总财力。
--
-- 原版 `Player` 的三个字段（`data_d/game_7010544/sources/bf/game/server/module/player/Player.java`）：
--   `:146` `@Column(name = "money", length = Coliseum.MAX_INDEX)` —— `String money`，银两
--   `:266` `@Column(name = "totalMoney")` —— `BigDecimal totalMoney`，总财力
--   `:269` `@Column(name = "maxTotalMoney")` —— `BigDecimal maxTotalMoney`，历史最大财力
--
-- `money` 在原版是**字符串**（`BigInteger` 的十进制表示，见 `Player.java:1033 addMoney`
-- 与 `:1097 removeMoney`），可以超过 i64（远洋贸易 400 号关卡 cost 就是
-- `4680000000000000000`，仍 < 9.22e18，但升级手续费会更大），所以这里也用
-- `NUMERIC(40,0)` 而不是 `BIGINT`。
--
-- `totalMoney` / `maxTotalMoney` 在原版是 `BigDecimal`（`Player.java:1857`），
-- 由 `refreshTotalMoney()`（`:1862`）从 `TalentSet` + `ChildrenSet` 汇总。
-- 移植阶段 `TalentSet` / `ChildrenSet` 都还没接（见 AUDIT A22 的未接线表），
-- 所以这两列先建出来但写 0。

ALTER TABLE player ADD COLUMN IF NOT EXISTS money NUMERIC(40, 0) NOT NULL DEFAULT 0;
ALTER TABLE player ADD COLUMN IF NOT EXISTS total_money NUMERIC(40, 0) NOT NULL DEFAULT 0;
ALTER TABLE player ADD COLUMN IF NOT EXISTS max_total_money NUMERIC(40, 0) NOT NULL DEFAULT 0;
