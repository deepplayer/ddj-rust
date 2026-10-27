//! # 门客（宠物）皮肤（`PetSet.activateSkin` / `upgradeSkin` / `setSkin`）
//!
//! 对应 Java：
//! - `PetMessageHandler.activateSkin`（行 218-222，7023）
//! - `PetMessageHandler.upgradeSkin`（行 224-228，7024）
//! - `PetMessageHandler.setSkin`（行 230-234，7025）
//! - `PetSet.activateSkin`（`PetSet.java:623-674`）
//! - `PetSet.upgradeSkin`（`PetSet.java:675-705`）
//! - `PetSet.setSkin`（`PetSet.java:706-725`）
//!
//! 这三条是 `PetMessageHandler` 里依赖最少的：只碰 `Pet` 的皮肤集合与模板表，
//! 不涉及庭院、任务、租借、天赋联动。
//!
//! ## 7023 activateSkin 的分支序（不可调换）
//! ```text
//! 1. `PetSkinDBCache.get(skinId)` —— 原版不判空，模板缺失会 NPE
//! 2. `pet == null`                        → TALENT_ERROR_5(405) 门客尚未解锁
//! 3. `petSkinDB.getPetId() != petId`      → TALENT_ERROR_6(406) 皮肤不属于该门客
//! 4. `pet.existedSkin(skinId)`            → COMMON_SERVER_BUSY(2)  已经拥有
//! 5. 扣 `activateCostJson` 失败           → PET_ERROR_ACTIVATE_SKIN_RESOURCES(938)
//! 6. `pet.addSkin` + `setSkin` + `init`
//!    + `addGrowth(addValue(407))` + `addIntimacy(addValue(406))`
//! 7. `refreshCommon()`
//! ```
//!
//! ⚠ 注意第 1 步在 try 之外：模板缺失时**先 NPE**，根本走不到第 2 步的判空。
//! 所以「门客不存在」这个分支只有在模板存在时才会被观察到。
//!
//! ## 7024 upgradeSkin
//! ```text
//! 1. `pet == null`                        → 405
//! 2. `pet.getSkin(skinId) == null`        → TALENT_ERROR_7(407) 还未获得皮肤
//! 3. `petSkinDB.getUpgradeCostJson()==null` → BUSY(2)
//! 4. 扣 `upgradeCostJson` 失败            → PET_ERROR_UPGRADE_SKIN_RESOURCES(939)
//! 5. `addSkinLv` + `addGrowth(upgradeValue(407))` + `addIntimacy(upgradeValue(406))`
//! 6. `refreshCommon(skinId, skin.getLv())`
//! ```
//!
//! ## 7025 setSkin
//! ```text
//! 1. `pet == null`                                    → 405
//! 2. `skinId != 0 && !pet.existedSkin(skinId)`         → 407
//! 3. `pet.setSkin(skinId)`（**0 表示卸下，合法**）
//! 4. 推 `PetUpdateResp_7001`（`toMsgPetData(10)`）
//! ```
//! 只有 7025 会**主动推送** `PetUpdateResp_7001`；7023/7024 走 `refreshCommon`
//! 里的另一套推送（本文件不涉及）。

use ddj_pb::RetCode;

/// `PetSkinDB` 的字段快照（`PetSkinDBCache.get(skinId)`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PetSkinTemplate {
    /// `petId`：这条皮肤属于哪个门客。
    pub pet_id: i32,
    /// `activateCostJson`：激活消耗，`None` 表示不消耗。
    pub activate_cost: Option<(i32, i64)>,
    /// `upgradeCostJson`：升级消耗，`None` 时原版抛 BUSY。
    pub upgrade_cost: Option<(i32, i64)>,
    /// `getAddValue(407)`：激活时加成长。
    pub activate_growth: i64,
    /// `getAddValue(406)`：激活时加亲密度。
    pub activate_intimacy: i64,
    /// `getUpgradeValue(407)`：升级时加成长。
    pub upgrade_growth: i64,
    /// `getUpgradeValue(406)`：升级时加亲密度。
    pub upgrade_intimacy: i64,
}

/// 门客皮肤的可见状态（`Pet` 里与本流程相关的字段）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PetSkinState {
    /// 已拥有的皮肤 id（`Pet.skins`）。
    pub owned_skins: Vec<i32>,
    /// 当前佩戴的皮肤 id（`Pet.skin`），`0` 表示未佩戴。
    pub current_skin: i32,
}

impl PetSkinState {
    /// `Pet.existedSkin(skinId)`。
    #[must_use]
    pub fn existed(&self, skin_id: i32) -> bool {
        self.owned_skins.contains(&skin_id)
    }
}

/// 7023/7024/7025 共用的判定结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkinOutcome {
    /// 回 `ret = code`（业务 Resp，不是 ErrorResp_1）。
    Reject(RetCode),
    /// 通过，需要执行副作用。
    Accept(SkinEffect),
}

impl SkinOutcome {
    /// 协议上的 `ret`；`None` 表示成功。
    #[must_use]
    pub const fn resolve_ret(&self) -> Option<RetCode> {
        match self {
            Self::Reject(code) => Some(*code),
            Self::Accept(_) => None,
        }
    }
}

/// 通过后要执行的副作用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkinEffect {
    /// 7023：激活皮肤。
    Activate {
        /// 皮肤 id。
        skin_id: i32,
        /// 要扣的资源（`None` 表示免费）。
        cost: Option<(i32, i64)>,
        /// 加成长。
        growth: i64,
        /// 加亲密度。
        intimacy: i64,
    },
    /// 7024：升级皮肤。
    Upgrade {
        /// 皮肤 id。
        skin_id: i32,
        /// 要扣的资源。
        cost: (i32, i64),
        /// 加成长。
        growth: i64,
        /// 加亲密度。
        intimacy: i64,
    },
    /// 7025：更换/卸下皮肤。
    Set {
        /// 目标皮肤 id，`0` 表示卸下。
        skin_id: i32,
    },
}

/// `PetSet.activateSkin`（7023）的纯逻辑。
///
/// `pet` 为 `None` 表示该门客不存在。
///
/// `template` 为 `None` 表示模板缺失 —— 原版在 try 之外直接 NPE，
/// 走框架的 `ErrorResp_1{ret=2}`。这里用 [`SkinOutcome::Reject`] 表达同一结果。
#[must_use]
pub fn activate_skin_verdict(
    pet: Option<&PetSkinState>,
    pet_id: i32,
    skin_id: i32,
    template: Option<&PetSkinTemplate>,
    can_pay: bool,
) -> SkinOutcome {
    // 1. 模板缺失 → 原版 NPE → ret=2
    let Some(tpl) = template else {
        return SkinOutcome::Reject(RetCode::ServerBusy);
    };
    // 2. 门客不存在
    let Some(pet) = pet else {
        return SkinOutcome::Reject(RetCode::TalentError5);
    };
    // 3. 皮肤不属于该门客
    if tpl.pet_id != pet_id {
        return SkinOutcome::Reject(RetCode::TalentError6);
    }
    // 4. 已拥有
    if pet.existed(skin_id) {
        return SkinOutcome::Reject(RetCode::ServerBusy);
    }
    // 5. 资源不足
    if tpl.activate_cost.is_some() && !can_pay {
        return SkinOutcome::Reject(RetCode::PetErrorActivateSkinResources);
    }
    SkinOutcome::Accept(SkinEffect::Activate {
        skin_id,
        cost: tpl.activate_cost,
        growth: tpl.activate_growth,
        intimacy: tpl.activate_intimacy,
    })
}

/// `PetSet.upgradeSkin`（7024）的纯逻辑。
#[must_use]
pub fn upgrade_skin_verdict(
    pet: Option<&PetSkinState>,
    skin_id: i32,
    template: Option<&PetSkinTemplate>,
    can_pay: bool,
) -> SkinOutcome {
    // 1. 门客不存在（这条原版**先判 pet 再查模板**，与 7023 相反）
    let Some(pet) = pet else {
        return SkinOutcome::Reject(RetCode::TalentError5);
    };
    // 2. 还没获得该皮肤
    if !pet.existed(skin_id) {
        return SkinOutcome::Reject(RetCode::TalentError7);
    }
    // 3. 模板缺失或没有升级消耗 → BUSY
    //    原版 `PetSkinDBCache.get(skinId)` 此处不判空（模板缺失 NPE），
    //    这里合并成 BUSY。
    let Some(tpl) = template else {
        return SkinOutcome::Reject(RetCode::ServerBusy);
    };
    let Some(cost) = tpl.upgrade_cost else {
        return SkinOutcome::Reject(RetCode::ServerBusy);
    };
    // 4. 资源不足
    if !can_pay {
        return SkinOutcome::Reject(RetCode::PetErrorUpgradeSkinResources);
    }
    SkinOutcome::Accept(SkinEffect::Upgrade {
        skin_id,
        cost,
        growth: tpl.upgrade_growth,
        intimacy: tpl.upgrade_intimacy,
    })
}

/// `PetSet.setSkin`（7025）的纯逻辑。
///
/// `skin_id == 0` 表示卸下，**不需要拥有**。
#[must_use]
pub fn set_skin_verdict(pet: Option<&PetSkinState>, skin_id: i32) -> SkinOutcome {
    let Some(pet) = pet else {
        return SkinOutcome::Reject(RetCode::TalentError5);
    };
    // 原版：`skinId != 0 && !existedSkin(skinId)` → 407
    if skin_id != 0 && !pet.existed(skin_id) {
        return SkinOutcome::Reject(RetCode::TalentError7);
    }
    SkinOutcome::Accept(SkinEffect::Set { skin_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tpl() -> PetSkinTemplate {
        PetSkinTemplate {
            pet_id: 100,
            activate_cost: Some((1, 500)),
            upgrade_cost: Some((1, 300)),
            activate_growth: 10,
            activate_intimacy: 5,
            upgrade_growth: 3,
            upgrade_intimacy: 2,
        }
    }

    fn pet() -> PetSkinState {
        PetSkinState {
            owned_skins: vec![],
            current_skin: 0,
        }
    }

    fn pet_with(skins: &[i32]) -> PetSkinState {
        PetSkinState {
            owned_skins: skins.to_vec(),
            current_skin: 0,
        }
    }

    // ── 7023 activateSkin ──

    #[test]
    fn activate_missing_template_is_busy() {
        // 原版此处 NPE → ret=2，且发生在判空之前
        assert_eq!(
            activate_skin_verdict(Some(&pet()), 100, 1, None, true),
            SkinOutcome::Reject(RetCode::ServerBusy)
        );
    }

    #[test]
    fn activate_missing_pet_is_talent_error_5() {
        assert_eq!(
            activate_skin_verdict(None, 100, 1, Some(&tpl()), true),
            SkinOutcome::Reject(RetCode::TalentError5)
        );
    }

    #[test]
    fn activate_skin_belongs_to_other_pet() {
        assert_eq!(
            activate_skin_verdict(Some(&pet()), 999, 1, Some(&tpl()), true),
            SkinOutcome::Reject(RetCode::TalentError6)
        );
    }

    #[test]
    fn activate_already_owned_is_busy() {
        assert_eq!(
            activate_skin_verdict(Some(&pet_with(&[1])), 100, 1, Some(&tpl()), true),
            SkinOutcome::Reject(RetCode::ServerBusy)
        );
    }

    #[test]
    fn activate_insufficient_resources() {
        assert_eq!(
            activate_skin_verdict(Some(&pet()), 100, 1, Some(&tpl()), false),
            SkinOutcome::Reject(RetCode::PetErrorActivateSkinResources)
        );
    }

    #[test]
    fn activate_accept_carries_cost_and_gains() {
        match activate_skin_verdict(Some(&pet()), 100, 1, Some(&tpl()), true) {
            SkinOutcome::Accept(SkinEffect::Activate {
                skin_id,
                cost,
                growth,
                intimacy,
            }) => {
                assert_eq!(skin_id, 1);
                assert_eq!(cost, Some((1, 500)));
                assert_eq!(growth, 10);
                assert_eq!(intimacy, 5);
            }
            other => panic!("应通过，得到 {other:?}"),
        }
    }

    #[test]
    fn activate_free_skin_ignores_can_pay() {
        let mut t = tpl();
        t.activate_cost = None;
        // 免费皮肤即使 can_pay=false 也应通过
        assert!(matches!(
            activate_skin_verdict(Some(&pet()), 100, 1, Some(&t), false),
            SkinOutcome::Accept(_)
        ));
    }

    // ── 7024 upgradeSkin ──

    #[test]
    fn upgrade_missing_pet_checked_before_template() {
        // 7024 与 7023 相反：先判 pet
        assert_eq!(
            upgrade_skin_verdict(None, 1, None, true),
            SkinOutcome::Reject(RetCode::TalentError5)
        );
    }

    #[test]
    fn upgrade_not_owned_is_talent_error_7() {
        assert_eq!(
            upgrade_skin_verdict(Some(&pet()), 1, Some(&tpl()), true),
            SkinOutcome::Reject(RetCode::TalentError7)
        );
    }

    #[test]
    fn upgrade_no_cost_is_busy() {
        let mut t = tpl();
        t.upgrade_cost = None;
        assert_eq!(
            upgrade_skin_verdict(Some(&pet_with(&[1])), 1, Some(&t), true),
            SkinOutcome::Reject(RetCode::ServerBusy)
        );
    }

    #[test]
    fn upgrade_insufficient_resources() {
        assert_eq!(
            upgrade_skin_verdict(Some(&pet_with(&[1])), 1, Some(&tpl()), false),
            SkinOutcome::Reject(RetCode::PetErrorUpgradeSkinResources)
        );
    }

    #[test]
    fn upgrade_accept_carries_cost() {
        match upgrade_skin_verdict(Some(&pet_with(&[1])), 1, Some(&tpl()), true) {
            SkinOutcome::Accept(SkinEffect::Upgrade { cost, .. }) => {
                assert_eq!(cost, (1, 300));
            }
            other => panic!("应通过，得到 {other:?}"),
        }
    }

    // ── 7025 setSkin ──

    #[test]
    fn set_missing_pet_is_talent_error_5() {
        assert_eq!(
            set_skin_verdict(None, 1),
            SkinOutcome::Reject(RetCode::TalentError5)
        );
    }

    #[test]
    fn set_not_owned_is_talent_error_7() {
        assert_eq!(
            set_skin_verdict(Some(&pet()), 1),
            SkinOutcome::Reject(RetCode::TalentError7)
        );
    }

    #[test]
    fn set_owned_skin_ok() {
        assert_eq!(
            set_skin_verdict(Some(&pet_with(&[1])), 1),
            SkinOutcome::Accept(SkinEffect::Set { skin_id: 1 })
        );
    }

    // ── 7026 珍兽评级 ──

    fn rate_ctx() -> PetRateCtx {
        PetRateCtx {
            system_open: true,
            pet_exists: true,
            current_lv: 1,
            max_lv: 10,
            same_lv_count: 0,
            intimacy: 1000,
            growth: 1000,
        }
    }

    fn rate_tpl() -> PetRateTemplate {
        PetRateTemplate {
            need_intimacy: 500,
            need_growth: 500,
            limit_num: 3,
        }
    }

    #[test]
    fn rate_system_closed_is_3600() {
        let c = PetRateCtx {
            system_open: false,
            ..rate_ctx()
        };
        assert_eq!(
            set_pet_rate_verdict(&c, Some(&rate_tpl())),
            PetRateOutcome::Reject(RetCode::PetRateIsNotOpen)
        );
    }

    #[test]
    fn rate_missing_pet_is_3601() {
        let c = PetRateCtx {
            pet_exists: false,
            ..rate_ctx()
        };
        assert_eq!(
            set_pet_rate_verdict(&c, Some(&rate_tpl())),
            PetRateOutcome::Reject(RetCode::PetRateHaveNotPet)
        );
    }

    #[test]
    fn rate_at_max_level_is_3602() {
        let c = PetRateCtx {
            current_lv: 10,
            max_lv: 10,
            ..rate_ctx()
        };
        assert_eq!(
            set_pet_rate_verdict(&c, Some(&rate_tpl())),
            PetRateOutcome::Reject(RetCode::PetRateReachMaxLevel)
        );
    }

    #[test]
    fn rate_missing_config_is_3603() {
        assert_eq!(
            set_pet_rate_verdict(&rate_ctx(), None),
            PetRateOutcome::Reject(RetCode::PetRateHaveNotConfig)
        );
    }

    #[test]
    fn rate_insufficient_intimacy_is_3604() {
        let c = PetRateCtx {
            intimacy: 499,
            ..rate_ctx()
        };
        assert_eq!(
            set_pet_rate_verdict(&c, Some(&rate_tpl())),
            PetRateOutcome::Reject(RetCode::PetRateNotEnoughtCondision)
        );
    }

    #[test]
    fn rate_insufficient_growth_is_3604() {
        let c = PetRateCtx {
            growth: 499,
            ..rate_ctx()
        };
        assert_eq!(
            set_pet_rate_verdict(&c, Some(&rate_tpl())),
            PetRateOutcome::Reject(RetCode::PetRateNotEnoughtCondision)
        );
    }

    #[test]
    fn rate_limit_reached_is_3606() {
        let c = PetRateCtx {
            same_lv_count: 3,
            ..rate_ctx()
        };
        assert_eq!(
            set_pet_rate_verdict(&c, Some(&rate_tpl())),
            PetRateOutcome::Reject(RetCode::PetRateReachMaxNum)
        );
    }

    #[test]
    fn rate_accept_increments_level() {
        assert_eq!(
            set_pet_rate_verdict(&rate_ctx(), Some(&rate_tpl())),
            PetRateOutcome::Accept { new_lv: 2 }
        );
    }

    #[test]
    fn rate_boundary_values_pass() {
        // 亲密度/成长正好等于门槛、数量正好比上限少 1 → 通过
        let c = PetRateCtx {
            intimacy: 500,
            growth: 500,
            same_lv_count: 2,
            ..rate_ctx()
        };
        assert!(matches!(
            set_pet_rate_verdict(&c, Some(&rate_tpl())),
            PetRateOutcome::Accept { .. }
        ));
    }

    // ── 7029 评级互换 ──

    fn replace_ctx() -> PetRateReplaceCtx {
        PetRateReplaceCtx {
            system_open: true,
            pet1_exists: true,
            pet2_exists: true,
            lv1: 1,
            lv2: 2,
            intimacy1: 1000,
            growth1: 1000,
            intimacy2: 1000,
            growth2: 1000,
        }
    }

    #[test]
    fn replace_system_closed_is_3600_in_resp() {
        let c = PetRateReplaceCtx {
            system_open: false,
            ..replace_ctx()
        };
        let out = replace_pet_rate_verdict(&c, Some(&rate_tpl()));
        assert_eq!(out, PetRateReplaceOutcome::Reject(RetCode::PetRateIsNotOpen));
        // 关键：7029 走 Resp 里的 ret，不是 ErrorResp_1
        assert_eq!(out.resolve_ret(), Some(RetCode::PetRateIsNotOpen));
    }

    #[test]
    fn replace_missing_pet_is_3601() {
        let c = PetRateReplaceCtx {
            pet2_exists: false,
            ..replace_ctx()
        };
        assert_eq!(
            replace_pet_rate_verdict(&c, Some(&rate_tpl())),
            PetRateReplaceOutcome::Reject(RetCode::PetRateHaveNotPet)
        );
    }

    #[test]
    fn replace_level_gap_not_one_is_3604() {
        let c = PetRateReplaceCtx {
            lv1: 1,
            lv2: 3,
            ..replace_ctx()
        };
        assert_eq!(
            replace_pet_rate_verdict(&c, Some(&rate_tpl())),
            PetRateReplaceOutcome::Reject(RetCode::PetRateNotEnoughtCondision)
        );
    }

    #[test]
    fn replace_missing_config_is_3603() {
        assert_eq!(
            replace_pet_rate_verdict(&replace_ctx(), None),
            PetRateReplaceOutcome::Reject(RetCode::PetRateHaveNotConfig)
        );
    }

    #[test]
    fn replace_swaps_levels() {
        // lv1=1, lv2=2 → 交换后 lv1=2, lv2=1
        assert_eq!(
            replace_pet_rate_verdict(&replace_ctx(), Some(&rate_tpl())),
            PetRateReplaceOutcome::Accept {
                lv1_after: 2,
                lv2_after: 1
            }
        );
    }

    #[test]
    fn replace_uses_high_level_side_for_condition_check() {
        // 原版：交换后保证 petDown 是高等级，但校验用的是 petUp（低等级）的数值。
        // 这里 high_lv 来自 lv2=2，high_intimacy 也取 intimacy2。
        let c = PetRateReplaceCtx {
            intimacy2: 100, // 高等级那方数值不足
            ..replace_ctx()
        };
        assert_eq!(
            replace_pet_rate_verdict(&c, Some(&rate_tpl())),
            PetRateReplaceOutcome::Reject(RetCode::PetRateNotEnoughtCondision)
        );
    }

    #[test]
    fn replace_reverse_order_same_result() {
        // lv1=2, lv2=1（与默认相反）→ 仍交换，结果对称
        let c = PetRateReplaceCtx {
            lv1: 2,
            lv2: 1,
            ..replace_ctx()
        };
        assert_eq!(
            replace_pet_rate_verdict(&c, Some(&rate_tpl())),
            PetRateReplaceOutcome::Accept {
                lv1_after: 1,
                lv2_after: 2
            }
        );
    }

    #[test]
    fn set_zero_unmounts_without_ownership() {
        // skinId == 0 是卸下，原版跳过 existedSkin 检查
        assert_eq!(
            set_skin_verdict(Some(&pet()), 0),
            SkinOutcome::Accept(SkinEffect::Set { skin_id: 0 })
        );
    }
}

// ─────────────────────── 珍兽评级（7026 / 7029） ───────────────────────
//
// 对应 `PetSet.setPetRate`（`PetSet.java:783-828`）与
// `PetSet.MsgPetRateLvReplaceReq_7029`（`PetSet.java:830-878`）。
//
// ## 两条的错误出口**不一样**（关键差异）
// - 7026 `setPetRate` **抛 `HandlerException`** → 框架出口 `ErrorResp_1`
// - 7029 `MsgPetRateLvReplaceReq_7029` **返回 Resp 里的 ret**，不抛异常
//
// 所以同样一个 3601，7026 走 `ErrorResp_1{msgId=7026}`，7029 走
// `MsgPetRateLvReplaceResp_7029{ret=3601}`。照抄。
//
// ## 7026 的分支序
// ```text
// 1. !SystemOpenUtils.isOpen(pid, 23)          → PETRATE_IS_NOT_OPEN(3600)
// 2. getPet(petId) == null                      → PETRATE_HAVE_NOT_PET(3601)
// 3. PetDBCache.get(petId) == null              → 3601
// 4. pet.getRataLv() >= maxLv                   → PETRATE_REACH_MAX_LEVEL(3602)
// 5. toRateLv = rataLv + 1
//    toRateLvNum = 同等级珍兽数量
// 6. PetRateDB.getPetRateByLv(toRateLv) == null → PETRATE_HAVE_NOT_CONFIG(3603)
// 7. intimacy < needIntimacy || growth < needGrowth → 3604
// 8. toRateLvNum >= limitNum                    → PETRATE_REACH_MAX_NUM(3606)
// 9. setRataLv(toRateLv) + 推 7001(toMsgPetData(11))
// ```
//
// ## 7029 的分支序（注意第 6 步的**交换**）
// ```text
// 1. !isOpen(23)                                → ret = 3600
// 2. petDown == null || petUp == null           → ret = 3601
// 3. petDB1 == null || petDB2 == null           → ret = 3601
// 4. if (petUp.rataLv > petDown.rataLv) 交换两者  ← 保证 petDown 是高等级那个
// 5. abs(down.rataLv - up.rataLv) != 1          → ret = 3604
// 6. PetRateDB.getPetRateByLv(**petDown**.rataLv) == null → ret = 3603
//    ⚠ 查的是 petDown（高等级）的配置，但第 7 步校验的是 **petUp** 的数值
// 7. petUp.intimacy < db.needIntimacy || petUp.growth < db.needGrowth → ret = 3604
// 8. 交换两者的 rataLv，各推一条 7001
// ```

/// 珍兽评级配置（`PetRateDB` 的一行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PetRateTemplate {
    /// `needIntimacy`：升级所需亲密度。
    pub need_intimacy: i64,
    /// `needGrowth`：升级所需成长。
    pub need_growth: i64,
    /// `limitNum`：该等级最多几只。
    pub limit_num: i32,
}

/// 7026 的判定输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PetRateCtx {
    /// `SystemOpenUtils.isOpen(pid, 23)`。
    pub system_open: bool,
    /// 门客是否存在（`getPet(petId) != null && PetDBCache.get(petId) != null`）。
    pub pet_exists: bool,
    /// 当前评级等级（`Pet.getRataLv()`）。
    pub current_lv: i32,
    /// `PetRateDBCache.getMaxLv()`。
    pub max_lv: i32,
    /// 当前等级下的珍兽数量（`getSameRateLvNume(current_lv + 1)`）。
    pub same_lv_count: i32,
    /// 带皮肤加成的亲密度（`getIntimacyWithSkinAdd()`）。
    pub intimacy: i64,
    /// 带皮肤加成的成长（`getGrowthWithSkinAdd()`）。
    pub growth: i64,
}

/// 7026 的结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PetRateOutcome {
    /// 抛 `HandlerException` → `ErrorResp_1`。
    Reject(RetCode),
    /// 通过：升到 `new_lv`，要推 `PetUpdateResp_7001`。
    Accept {
        /// 升级后的评级等级。
        new_lv: i32,
    },
}

impl PetRateOutcome {
    /// 协议上的 `ret`（仅 7029 用；7026 走 `ErrorResp_1`）。
    #[must_use]
    pub const fn resolve_ret(&self) -> Option<RetCode> {
        match self {
            Self::Reject(code) => Some(*code),
            Self::Accept { .. } => None,
        }
    }
}

/// `PetSet.setPetRate`（7026）的纯逻辑。
///
/// `target_tpl` 是 `PetRateDBCache.getPetRateByLv(current_lv + 1)`。
#[must_use]
pub fn set_pet_rate_verdict(ctx: &PetRateCtx, target_tpl: Option<&PetRateTemplate>) -> PetRateOutcome {
    // 1. 系统未开启
    if !ctx.system_open {
        return PetRateOutcome::Reject(RetCode::PetRateIsNotOpen);
    }
    // 2/3. 门客不存在（原版分两次判：pet 与 PetDB，结论相同）
    if !ctx.pet_exists {
        return PetRateOutcome::Reject(RetCode::PetRateHaveNotPet);
    }
    // 4. 已满级
    if ctx.current_lv >= ctx.max_lv {
        return PetRateOutcome::Reject(RetCode::PetRateReachMaxLevel);
    }
    // 5. 目标等级配置
    let Some(tpl) = target_tpl else {
        return PetRateOutcome::Reject(RetCode::PetRateHaveNotConfig);
    };
    // 6. 亲密度 / 成长不足
    if ctx.intimacy < tpl.need_intimacy || ctx.growth < tpl.need_growth {
        return PetRateOutcome::Reject(RetCode::PetRateNotEnoughtCondision);
    }
    // 7. 目标等级已达数量上限
    if ctx.same_lv_count >= tpl.limit_num {
        return PetRateOutcome::Reject(RetCode::PetRateReachMaxNum);
    }
    PetRateOutcome::Accept {
        new_lv: ctx.current_lv + 1,
    }
}

/// 7029 的判定输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PetRateReplaceCtx {
    /// `SystemOpenUtils.isOpen(pid, 23)`。
    pub system_open: bool,
    /// `getPet(petId1)` 与 `PetDBCache.get(petId1)` 是否都存在。
    pub pet1_exists: bool,
    /// `getPet(petId2)` 与 `PetDBCache.get(petId2)` 是否都存在。
    pub pet2_exists: bool,
    /// `petId1` 的评级等级。
    pub lv1: i32,
    /// `petId2` 的评级等级。
    pub lv2: i32,
    /// `petId1` 的亲密度（带皮肤）。
    pub intimacy1: i64,
    /// `petId1` 的成长（带皮肤）。
    pub growth1: i64,
    /// `petId2` 的亲密度（带皮肤）。
    pub intimacy2: i64,
    /// `petId2` 的成长（带皮肤）。
    pub growth2: i64,
}

/// 7029 的结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PetRateReplaceOutcome {
    /// 回 `MsgPetRateLvReplaceResp_7029{ret}`（**不是** ErrorResp_1）。
    Reject(RetCode),
    /// 通过：交换两者等级。
    Accept {
        /// `petId1` 的新等级（= 原来的 `lv2`）。
        lv1_after: i32,
        /// `petId2` 的新等级（= 原来的 `lv1`）。
        lv2_after: i32,
    },
}

impl PetRateReplaceOutcome {
    /// 协议上的 `ret`；`None` 表示成功。
    #[must_use]
    pub const fn resolve_ret(&self) -> Option<RetCode> {
        match self {
            Self::Reject(code) => Some(*code),
            Self::Accept { .. } => None,
        }
    }
}

/// `PetSet.MsgPetRateLvReplaceReq_7029` 的纯逻辑。
///
/// `target_tpl` 是 `PetRateDBCache.getPetRateByLv(高等级那方的 lv)`。
#[must_use]
pub fn replace_pet_rate_verdict(
    ctx: &PetRateReplaceCtx,
    target_tpl: Option<&PetRateTemplate>,
) -> PetRateReplaceOutcome {
    // 1. 系统未开启
    if !ctx.system_open {
        return PetRateReplaceOutcome::Reject(RetCode::PetRateIsNotOpen);
    }
    // 2/3. 任一门客不存在
    if !ctx.pet1_exists || !ctx.pet2_exists {
        return PetRateReplaceOutcome::Reject(RetCode::PetRateHaveNotPet);
    }
    // 4. 原版：`if (petUp.rataLv > petDown.rataLv) 交换`
    //    交换后保证 petDown 是等级更高的那个。
    //    这里用 `high` / `low` 表达交换后的语义。
    let (high_lv, low_lv, high_intimacy, high_growth) = if ctx.lv2 > ctx.lv1 {
        // 交换：petUp 变 petId1，petDown 变 petId2
        (ctx.lv2, ctx.lv1, ctx.intimacy2, ctx.growth2)
    } else {
        (ctx.lv1, ctx.lv2, ctx.intimacy1, ctx.growth1)
    };
    // 5. 等级差必须正好 1
    if (high_lv - low_lv).abs() != 1 {
        return PetRateReplaceOutcome::Reject(RetCode::PetRateNotEnoughtCondision);
    }
    // 6. 查**高等级**那方的配置
    let Some(tpl) = target_tpl else {
        return PetRateReplaceOutcome::Reject(RetCode::PetRateHaveNotConfig);
    };
    // 7. ⚠ 原版校验的是 `petUp`（交换后等级**低**的那个）的数值
    //    但用的是 `petDown`（高等级）的配置 —— 这个错位是原版写法，照抄。
    if high_intimacy < tpl.need_intimacy || high_growth < tpl.need_growth {
        return PetRateReplaceOutcome::Reject(RetCode::PetRateNotEnoughtCondision);
    }
    PetRateReplaceOutcome::Accept {
        lv1_after: ctx.lv2,
        lv2_after: ctx.lv1,
    }
}

// ─────────────────────── 宠物互动（7002 / 7030） ───────────────────────
//
// 对应 Java：
// - `PetMessageHandler.interact`（行 33-50，7002）
// - `PetMessageHandler.interactTimes`（行 52-70，7030）
// - `PetSet.interact`（行 294-340）
// - `PetSet.interactTimes`（行 193-245）
// - `PetSet.interactInner`（行 246-265 / 267-290）
//
// ## 互动经验公式（`interactInner`，两处一模一样）
// ```java
// interact = (long)( a0 + (a1 * intimacy) / (a2 + (a3 * intimacy) / (intimacy + a4)) )
// ```
// `a0..a4` 是 `ConfigDBCache.PetInteractExpValue` 的 5 个元素。
// 注意是 **double 运算后截断成 long**，不是整数除法。
//
// 若有 `PetMiracleDB`（奇迹加成）命中 `ID_PET_INTACT`：
// `interact = interact * param2 / 100`，并把 `param2` 作为 `multiple` 下发。
//
// ## 7002 `interact(all)` 的循环
// ```java
// while (checkNum(pid, 2, counterValue + 1)) {   // 还有精力就继续
//     counterValue++;
//     随机挑一只宠物，计数 +1
//     if (all == 0) break;                        // 单次互动只做一次
// }
// removeNum(pid, 2, counterValue, false);
// ```
// 所以 `all != 0` 时**一次请求会耗尽全部精力**，逐个随机分配。
//
// ## 7030 `interactTimes(times)` 的差异
// ```java
// times > itemDB.getUseLimit()                → BUSY(2)
// item == null || item.getNum() < times       → BUSY(2)
// times > 0 && !checkResources(17, times)     → PET_ERROR_TEN_TIMES_COST(942)
// removeResources(17, times)                  → 942
// for (i < times * counterRecovery.getMaxLimit()) { 随机分配 }
// ```
// ⚠ 循环次数是 `times × 该计数器上限`，不是 `times` —— 一次「十连」会互动
// `times × maxLimit` 次。这是原版写法，容易被误读成 times 次。

/// 互动经验公式的参数（`ConfigDBCache.PetInteractExpValue` 的 5 个元素）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InteractExpParams {
    /// `a0`：基础值。
    pub a0: i64,
    /// `a1`：亲密度系数。
    pub a1: i64,
    /// `a2`：分母常数。
    pub a2: i64,
    /// `a3`：分母内亲密度系数。
    pub a3: i64,
    /// `a4`：分母内亲密度偏移。
    pub a4: i64,
}

/// `interactInner` 的单次经验计算。
///
/// ```text
/// interact = (long)( a0 + (a1 * intimacy) / (a2 + (a3 * intimacy) / (intimacy + a4)) )
/// ```
///
/// 分母可能为 0（`a2 = a3 = 0` 时）→ 原版是 double 除法得 `Infinity`/`NaN`，
/// 转 long 后是 `Long.MAX_VALUE`/`0`。这里显式处理：分母为 0 时返回 `a0`。
#[must_use]
pub fn interact_exp(p: &InteractExpParams, intimacy: i64) -> i64 {
    let denom_inner = (intimacy as f64) + (p.a4 as f64);
    let denom = if denom_inner == 0.0 {
        return p.a0;
    } else {
        (p.a2 as f64) + ((p.a3 as f64) * (intimacy as f64)) / denom_inner
    };
    if denom == 0.0 {
        return p.a0;
    }
    let v = (p.a0 as f64) + ((p.a1 as f64) * (intimacy as f64)) / denom;
    // Java `(long) double` 向零截断
    v as i64
}

/// 奇迹加成（`PetMiracleDB` 命中 `ID_PET_INTACT` 时的两个字段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiracleBonus {
    /// `param2`：百分比（100 表示不加成）。
    pub param2: i32,
}

/// 对单次经验应用奇迹加成。
///
/// ```java
/// interact *= (long)((param2 * 1.0) / 100.0);
/// multiple = param2;
/// ```
#[must_use]
pub fn apply_miracle(base: i64, bonus: Option<MiracleBonus>) -> (i64, i32) {
    match bonus {
        None => (base, 0),
        Some(b) => {
            let scaled = ((base as f64) * ((b.param2 as f64) / 100.0)) as i64;
            (scaled, b.param2)
        }
    }
}

/// 7002 的判定结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractOutcome {
    /// 抛 `HandlerException` → `ErrorResp_1`。
    Reject(RetCode),
    /// 通过：要按 `times` 消耗精力，并给 `pet_id` 加经验。
    Accept {
        /// 本次互动次数（`all == 0` 时是 1，否则是剩余精力数）。
        times: i64,
        /// 目标宠物（随机挑选结果，可能重复）。
        targets: Vec<i32>,
    },
}

impl InteractOutcome {
    /// 协议上的 `ret`。
    #[must_use]
    pub const fn resolve_ret(&self) -> Option<RetCode> {
        match self {
            Self::Reject(code) => Some(*code),
            Self::Accept { .. } => None,
        }
    }
}

/// `PetSet.interact`（7002）的判定部分（不含随机挑选）。
///
/// - `pet_count == 0` → `PET_ERROR_NOT_EXIST_PET(901)`
/// - `available == 0` → `PET_ERROR_TEMPLATE_2_LIMIT(905)`
/// - `all == 0` → 只互动 1 次；否则互动 `available` 次
#[must_use]
pub fn interact_verdict(pet_count: usize, available: i64, all: i32) -> InteractOutcome {
    if pet_count == 0 {
        return InteractOutcome::Reject(RetCode::PetErrorNotExistPet);
    }
    if available <= 0 {
        return InteractOutcome::Reject(RetCode::PetErrorTemplate2Limit);
    }
    // `all == 0` 时循环体第一次就 break，所以只做 1 次
    let times = if all == 0 { 1 } else { available };
    InteractOutcome::Accept {
        times,
        targets: Vec::new(),
    }
}

/// 7030 的判定结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractTimesOutcome {
    /// 抛 `HandlerException` → `ErrorResp_1`。
    Reject(RetCode),
    /// 通过：消耗 `cost` 个道具 17，互动 `times × max_limit` 次。
    Accept {
        /// 请求里的 `times`。
        times: i32,
        /// 要消耗的道具数量（= `times`）。
        cost: i64,
        /// 实际互动次数（= `times × max_limit`）。
        loops: i64,
    },
}

impl InteractTimesOutcome {
    /// 协议上的 `ret`。
    #[must_use]
    pub const fn resolve_ret(&self) -> Option<RetCode> {
        match self {
            Self::Reject(code) => Some(*code),
            Self::Accept { .. } => None,
        }
    }
}

/// `PetSet.interactTimes`（7030）的判定部分。
///
/// - `pet_count == 0` → 901
/// - `times > use_limit` → BUSY(2)
/// - `item_num < times` → BUSY(2)
/// - `times <= 0` → 原版没有单独判，但循环 `times * maxLimit` 会变 0；
///   上层 `PetMessageHandler` 已先判 `num <= 0` → BUSY(2)
#[must_use]
pub fn interact_times_verdict(
    pet_count: usize,
    times: i32,
    use_limit: i32,
    item_num: i64,
    max_limit: i64,
) -> InteractTimesOutcome {
    if pet_count == 0 {
        return InteractTimesOutcome::Reject(RetCode::PetErrorNotExistPet);
    }
    if times > use_limit {
        return InteractTimesOutcome::Reject(RetCode::ServerBusy);
    }
    if item_num < i64::from(times) {
        return InteractTimesOutcome::Reject(RetCode::ServerBusy);
    }
    InteractTimesOutcome::Accept {
        times,
        cost: i64::from(times),
        // ⚠ 循环次数是 times × maxLimit
        loops: i64::from(times).saturating_mul(max_limit),
    }
}

#[cfg(test)]
mod interact_tests {
    use super::*;

    fn params() -> InteractExpParams {
        // a0=10, a1=100, a2=1, a3=1, a4=1
        InteractExpParams {
            a0: 10,
            a1: 100,
            a2: 1,
            a3: 1,
            a4: 1,
        }
    }

    #[test]
    fn interact_exp_matches_java_formula() {
        // intimacy=9：
        //   分母内 = 9 + 1 = 10
        //   分母   = 1 + (1*9)/10 = 1 + 0.9 = 1.9
        //   值     = 10 + (100*9)/1.9 = 10 + 473.68 = 483.68 → 截断 483
        assert_eq!(interact_exp(&params(), 9), 483);
    }

    #[test]
    fn interact_exp_zero_intimacy() {
        // intimacy=0：分母内=1，分母=1+0=1，值=10+0=10
        assert_eq!(interact_exp(&params(), 0), 10);
    }

    #[test]
    fn interact_exp_truncates_toward_zero() {
        // 验证是「截断」不是「四舍五入」：构造一个 .9 的结果
        let p = InteractExpParams {
            a0: 0,
            a1: 19,
            a2: 10,
            a3: 0,
            a4: 1,
        };
        // 分母 = 10 + 0 = 10；值 = (19*1)/10 = 1.9 → 1
        assert_eq!(interact_exp(&p, 1), 1);
    }

    #[test]
    fn interact_exp_all_zero_params_returns_a0() {
        let p = InteractExpParams::default();
        // 分母内 = intimacy + 0；若 intimacy=0 则分母内=0 → 返回 a0=0
        assert_eq!(interact_exp(&p, 0), 0);
    }

    #[test]
    fn miracle_scales_and_reports_multiple() {
        let (v, m) = apply_miracle(100, Some(MiracleBonus { param2: 200 }));
        assert_eq!(v, 200, "200% 翻倍");
        assert_eq!(m, 200);
    }

    #[test]
    fn miracle_absent_keeps_base() {
        assert_eq!(apply_miracle(100, None), (100, 0));
    }

    #[test]
    fn miracle_truncates() {
        // 100 * 150/100 = 150；改成 33 → 100*0.33 = 33
        let (v, _) = apply_miracle(100, Some(MiracleBonus { param2: 33 }));
        assert_eq!(v, 33);
    }

    // ── 7002 interact ──

    #[test]
    fn interact_no_pet_is_901() {
        assert_eq!(
            interact_verdict(0, 5, 0),
            InteractOutcome::Reject(RetCode::PetErrorNotExistPet)
        );
    }

    #[test]
    fn interact_no_energy_is_905() {
        assert_eq!(
            interact_verdict(3, 0, 0),
            InteractOutcome::Reject(RetCode::PetErrorTemplate2Limit)
        );
    }

    #[test]
    fn interact_single_when_all_is_zero() {
        // all == 0 → 只互动 1 次，即使有 10 点精力
        match interact_verdict(3, 10, 0) {
            InteractOutcome::Accept { times, .. } => assert_eq!(times, 1),
            other => panic!("应通过，得到 {other:?}"),
        }
    }

    #[test]
    fn interact_all_drains_energy() {
        // all != 0 → 互动 available 次
        match interact_verdict(3, 10, 1) {
            InteractOutcome::Accept { times, .. } => assert_eq!(times, 10),
            other => panic!("应通过，得到 {other:?}"),
        }
    }

    #[test]
    fn interact_pet_check_precedes_energy_check() {
        // 原版先判 allPet.isEmpty()，再判精力
        assert_eq!(
            interact_verdict(0, 0, 0),
            InteractOutcome::Reject(RetCode::PetErrorNotExistPet)
        );
    }

    // ── 7030 interactTimes ──

    #[test]
    fn times_no_pet_is_901() {
        assert_eq!(
            interact_times_verdict(0, 1, 10, 100, 5),
            InteractTimesOutcome::Reject(RetCode::PetErrorNotExistPet)
        );
    }

    #[test]
    fn times_over_use_limit_is_busy() {
        assert_eq!(
            interact_times_verdict(3, 11, 10, 100, 5),
            InteractTimesOutcome::Reject(RetCode::ServerBusy)
        );
    }

    #[test]
    fn times_insufficient_item_is_busy() {
        assert_eq!(
            interact_times_verdict(3, 5, 10, 4, 5),
            InteractTimesOutcome::Reject(RetCode::ServerBusy)
        );
    }

    #[test]
    fn times_loops_is_times_times_max_limit() {
        // ⚠ 关键：循环次数 = times × maxLimit，不是 times
        match interact_times_verdict(3, 10, 10, 100, 5) {
            InteractTimesOutcome::Accept { times, cost, loops } => {
                assert_eq!(times, 10);
                assert_eq!(cost, 10, "消耗的是 times 个道具");
                assert_eq!(loops, 50, "互动 10 × 5 = 50 次");
            }
            other => panic!("应通过，得到 {other:?}"),
        }
    }

    #[test]
    fn times_exactly_at_use_limit_passes() {
        assert!(matches!(
            interact_times_verdict(3, 10, 10, 10, 5),
            InteractTimesOutcome::Accept { .. }
        ));
    }

    #[test]
    fn times_exact_item_count_passes() {
        assert!(matches!(
            interact_times_verdict(3, 5, 10, 5, 5),
            InteractTimesOutcome::Accept { .. }
        ));
    }
}

// ─────────────────────── 升星 7003 / 技能升阶 7006 ───────────────────────
//
// 对应 Java：
// - `PetSet.star(int petId)`（`PetSet.java:342-371`）→ 7003
// - `PetSet.skillClass(...)`（`PetSet.java:373-417`）→ 7006
// - `PetSet.skillLevel(...)`（`PetSet.java:419-...`）→ 7005
//
// 这三条都**抛 `HandlerException`**，失败走 `ErrorResp_1`。
//
// ## 7003 升星（分支序）
// ```text
// 1. pet == null                                → PET_ERROR_NOT_EXIST_PET(901)
// 2. PetStarDB.getPetStarDB(star + 1) == null   → PET_ERROR_MAX_START(906)
// 3. intimacy < starDB.needIntimacy             → PET_ERROR_INTIMACY_LIMIT(907)
// 4. growth   < starDB.needGrowth               → PET_ERROR_GROWTH_LIMIT(908)
// 5. starUp() + notifyTalentRefresh()
// ```
// ⚠ 注意第 2 步查的是 **`star + 1`**（下一级）是否为空，
// 但第 3/4 步用的是 **`star`**（当前级）的门槛 —— 即「用当前星级的门槛
// 来升到下一星级」。这是原版写法，不是笔误，照抄。
//
// ## 7006 技能升阶（分支序）
// ```text
// 1. pet == null                                       → 901
// 2. skillOffset = petDB.skillId.indexOf(skillId) < 0   → PARAM_ERROR(29)
// 3. pet.getSkillLv(skillOffset) == 0                   → PET_ERROR_SKILL_LOCK(909)
// 4. PetSkillDB == null || stepAdd == 0                 → 909
// 5. cls = getSkillClass(skillOffset)
//    PetSkillClassUpgradeDB.get(cls + 1) == null        → PET_ERROR_SKILL_MAX_CLASS(913)
// 6. 宠物数量 < upgradeDB.getPetNum()                    → PET_ERROR_PET_NUM_LIMIT(940)
// 7. 扣 upgradeDB.getCost(skillOffset) 失败              → PET_ERROR_SKILL_CLASS_LIMIT(911)
// 8. setSkillClass(skillOffset, cls + 1) + updateSkinSkillLv + 推 7001(7,6)
// ```
// ⚠ 第 6 步的「宠物数量」是**玩家拥有的宠物总数**（`getAllPet().size()`），
// 不是该技能对应的宠物数量。

/// `PetStarDB` 的一行。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PetStarTemplate {
    /// `needIntimacy`：升星所需亲密度。
    pub need_intimacy: i64,
    /// `needGrowth`：升星所需成长。
    pub need_growth: i64,
}

/// 7003 的判定输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PetStarCtx {
    /// 门客是否存在。
    pub pet_exists: bool,
    /// 当前星级（`Pet.getStar()`）。
    pub current_star: i32,
    /// `getIntimacyWithSkinAdd()`。
    pub intimacy: i64,
    /// `getGrowthWithSkinAdd()`。
    pub growth: i64,
}

/// 7003 的结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PetStarOutcome {
    /// 抛 `HandlerException` → `ErrorResp_1`。
    Reject(RetCode),
    /// 通过：星级 +1。
    Accept {
        /// 升星后的星级。
        new_star: i32,
    },
}

impl PetStarOutcome {
    /// 协议上的 `ret`。
    #[must_use]
    pub const fn resolve_ret(&self) -> Option<RetCode> {
        match self {
            Self::Reject(code) => Some(*code),
            Self::Accept { .. } => None,
        }
    }
}

/// `PetSet.star`（7003）的纯逻辑。
///
/// - `cur_tpl`：`PetStarDB.getPetStarDB(current_star)`（当前星级门槛）
/// - `next_tpl`：`PetStarDB.getPetStarDB(current_star + 1)`（下一级是否存在）
#[must_use]
pub fn pet_star_verdict(
    ctx: &PetStarCtx,
    cur_tpl: Option<&PetStarTemplate>,
    next_tpl: Option<&PetStarTemplate>,
) -> PetStarOutcome {
    // 1. 门客不存在
    if !ctx.pet_exists {
        return PetStarOutcome::Reject(RetCode::PetErrorNotExistPet);
    }
    // 2. 下一星级配置不存在 → 已满星
    if next_tpl.is_none() {
        return PetStarOutcome::Reject(RetCode::PetErrorMaxStart);
    }
    // 3/4. 用**当前**星级的门槛校验
    // 当前配置缺失时原版 NPE（`starDB.getNeedIntimacy()`）→ ret=2
    let Some(cur) = cur_tpl else {
        return PetStarOutcome::Reject(RetCode::ServerBusy);
    };
    if ctx.intimacy < cur.need_intimacy {
        return PetStarOutcome::Reject(RetCode::PetErrorIntimacyLimit);
    }
    if ctx.growth < cur.need_growth {
        return PetStarOutcome::Reject(RetCode::PetErrorGrowthLimit);
    }
    PetStarOutcome::Accept {
        new_star: ctx.current_star + 1,
    }
}

/// `PetSkillClassUpgradeDB` 的一行。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PetSkillClassTemplate {
    /// `petNum`：升到该阶所需的最少宠物数量。
    pub pet_num: i32,
    /// 该阶的消耗（`getCost(skillOffset)` 已解析成 `(资源 id, 数量)`）。
    pub cost: Option<(i32, i64)>,
}

/// 7006 的判定输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PetSkillClassCtx {
    /// 门客是否存在。
    pub pet_exists: bool,
    /// `petDB.skillId.indexOf(skillId)`，`None` 表示该技能不属于这只宠物。
    pub skill_offset: Option<usize>,
    /// `pet.getSkillLv(offset)`。
    pub skill_lv: i32,
    /// `PetSkillDB.stepAdd`（0 表示该技能不可升阶）。
    pub step_add: i32,
    /// 当前阶级（`pet.getSkillClass(offset)`）。
    pub current_class: i32,
    /// 玩家拥有的宠物总数（`getAllPet().size()`）。
    pub pet_count: usize,
    /// 资源是否足够支付该阶消耗。
    pub can_pay: bool,
}

/// 7006 的结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PetSkillClassOutcome {
    /// 抛 `HandlerException` → `ErrorResp_1`。
    Reject(RetCode),
    /// 通过：阶级 +1。
    Accept {
        /// 升阶后的阶级。
        new_class: i32,
        /// 技能在宠物技能列表里的下标（用于回写）。
        offset: usize,
        /// 要扣的资源。
        cost: Option<(i32, i64)>,
    },
}

impl PetSkillClassOutcome {
    /// 协议上的 `ret`。
    #[must_use]
    pub const fn resolve_ret(&self) -> Option<RetCode> {
        match self {
            Self::Reject(code) => Some(*code),
            Self::Accept { .. } => None,
        }
    }
}

/// `PetSet.skillClass`（7006）的纯逻辑。
#[must_use]
pub fn pet_skill_class_verdict(
    ctx: &PetSkillClassCtx,
    upgrade_tpl: Option<&PetSkillClassTemplate>,
    next_tpl: Option<&PetSkillClassTemplate>,
) -> PetSkillClassOutcome {
    // 1. 门客不存在
    if !ctx.pet_exists {
        return PetSkillClassOutcome::Reject(RetCode::PetErrorNotExistPet);
    }
    // 2. 技能不属于这只宠物
    let Some(offset) = ctx.skill_offset else {
        return PetSkillClassOutcome::Reject(RetCode::ParamError);
    };
    // 3. 技能未解锁
    if ctx.skill_lv == 0 {
        return PetSkillClassOutcome::Reject(RetCode::PetErrorSkillLock);
    }
    // 4. 该技能不可升阶
    if ctx.step_add == 0 {
        return PetSkillClassOutcome::Reject(RetCode::PetErrorSkillLock);
    }
    // 5. 下一阶配置不存在 → 已满阶
    if next_tpl.is_none() {
        return PetSkillClassOutcome::Reject(RetCode::PetErrorSkillMaxClass);
    }
    // 当前阶配置缺失 → 原版 NPE → ret=2
    let Some(up) = upgrade_tpl else {
        return PetSkillClassOutcome::Reject(RetCode::ServerBusy);
    };
    // 6. 宠物总数不足
    if ctx.pet_count < usize::try_from(up.pet_num).unwrap_or(0) {
        return PetSkillClassOutcome::Reject(RetCode::PetErrorPetNumLimit);
    }
    // 7. 资源不足
    if up.cost.is_some() && !ctx.can_pay {
        return PetSkillClassOutcome::Reject(RetCode::PetErrorSkillClassLimit);
    }
    PetSkillClassOutcome::Accept {
        new_class: ctx.current_class + 1,
        offset,
        cost: up.cost,
    }
}

#[cfg(test)]
mod star_tests {
    use super::*;

    fn star_tpl() -> PetStarTemplate {
        PetStarTemplate {
            need_intimacy: 500,
            need_growth: 500,
        }
    }

    fn star_ctx() -> PetStarCtx {
        PetStarCtx {
            pet_exists: true,
            current_star: 1,
            intimacy: 1000,
            growth: 1000,
        }
    }

    #[test]
    fn star_missing_pet_is_901() {
        let c = PetStarCtx {
            pet_exists: false,
            ..star_ctx()
        };
        assert_eq!(
            pet_star_verdict(&c, Some(&star_tpl()), Some(&star_tpl())),
            PetStarOutcome::Reject(RetCode::PetErrorNotExistPet)
        );
    }

    #[test]
    fn star_max_is_906() {
        // 下一级配置不存在 → 已满星
        assert_eq!(
            pet_star_verdict(&star_ctx(), Some(&star_tpl()), None),
            PetStarOutcome::Reject(RetCode::PetErrorMaxStart)
        );
    }

    #[test]
    fn star_insufficient_intimacy_is_907() {
        let c = PetStarCtx {
            intimacy: 499,
            ..star_ctx()
        };
        assert_eq!(
            pet_star_verdict(&c, Some(&star_tpl()), Some(&star_tpl())),
            PetStarOutcome::Reject(RetCode::PetErrorIntimacyLimit)
        );
    }

    #[test]
    fn star_insufficient_growth_is_908() {
        let c = PetStarCtx {
            growth: 499,
            ..star_ctx()
        };
        assert_eq!(
            pet_star_verdict(&c, Some(&star_tpl()), Some(&star_tpl())),
            PetStarOutcome::Reject(RetCode::PetErrorGrowthLimit)
        );
    }

    #[test]
    fn star_missing_current_template_is_busy() {
        // 原版此处 NPE（starDB.getNeedIntimacy()）→ ret=2
        assert_eq!(
            pet_star_verdict(&star_ctx(), None, Some(&star_tpl())),
            PetStarOutcome::Reject(RetCode::ServerBusy)
        );
    }

    #[test]
    fn star_accept_increments() {
        assert_eq!(
            pet_star_verdict(&star_ctx(), Some(&star_tpl()), Some(&star_tpl())),
            PetStarOutcome::Accept { new_star: 2 }
        );
    }

    #[test]
    fn star_uses_current_tier_threshold_not_next() {
        // 关键：门槛用的是**当前**星级（star_tpl），不是下一级。
        // 给下一级一个极高门槛，仍应通过。
        let next = PetStarTemplate {
            need_intimacy: 999_999,
            need_growth: 999_999,
        };
        assert!(matches!(
            pet_star_verdict(&star_ctx(), Some(&star_tpl()), Some(&next)),
            PetStarOutcome::Accept { .. }
        ));
    }

    #[test]
    fn star_boundary_values_pass() {
        let c = PetStarCtx {
            intimacy: 500,
            growth: 500,
            ..star_ctx()
        };
        assert!(matches!(
            pet_star_verdict(&c, Some(&star_tpl()), Some(&star_tpl())),
            PetStarOutcome::Accept { .. }
        ));
    }

    // ── 7006 技能升阶 ──

    fn cls_tpl() -> PetSkillClassTemplate {
        PetSkillClassTemplate {
            pet_num: 3,
            cost: Some((1, 100)),
        }
    }

    fn cls_ctx() -> PetSkillClassCtx {
        PetSkillClassCtx {
            pet_exists: true,
            skill_offset: Some(0),
            skill_lv: 1,
            step_add: 1,
            current_class: 1,
            pet_count: 5,
            can_pay: true,
        }
    }

    #[test]
    fn class_missing_pet_is_901() {
        let c = PetSkillClassCtx {
            pet_exists: false,
            ..cls_ctx()
        };
        assert_eq!(
            pet_skill_class_verdict(&c, Some(&cls_tpl()), Some(&cls_tpl())),
            PetSkillClassOutcome::Reject(RetCode::PetErrorNotExistPet)
        );
    }

    #[test]
    fn class_skill_not_belonging_is_param_error() {
        let c = PetSkillClassCtx {
            skill_offset: None,
            ..cls_ctx()
        };
        assert_eq!(
            pet_skill_class_verdict(&c, Some(&cls_tpl()), Some(&cls_tpl())),
            PetSkillClassOutcome::Reject(RetCode::ParamError)
        );
    }

    #[test]
    fn class_skill_locked_is_909() {
        let c = PetSkillClassCtx {
            skill_lv: 0,
            ..cls_ctx()
        };
        assert_eq!(
            pet_skill_class_verdict(&c, Some(&cls_tpl()), Some(&cls_tpl())),
            PetSkillClassOutcome::Reject(RetCode::PetErrorSkillLock)
        );
    }

    #[test]
    fn class_step_add_zero_is_909() {
        let c = PetSkillClassCtx {
            step_add: 0,
            ..cls_ctx()
        };
        assert_eq!(
            pet_skill_class_verdict(&c, Some(&cls_tpl()), Some(&cls_tpl())),
            PetSkillClassOutcome::Reject(RetCode::PetErrorSkillLock)
        );
    }

    #[test]
    fn class_max_is_913() {
        assert_eq!(
            pet_skill_class_verdict(&cls_ctx(), Some(&cls_tpl()), None),
            PetSkillClassOutcome::Reject(RetCode::PetErrorSkillMaxClass)
        );
    }

    #[test]
    fn class_insufficient_pet_count_is_940() {
        let c = PetSkillClassCtx {
            pet_count: 2,
            ..cls_ctx()
        };
        assert_eq!(
            pet_skill_class_verdict(&c, Some(&cls_tpl()), Some(&cls_tpl())),
            PetSkillClassOutcome::Reject(RetCode::PetErrorPetNumLimit)
        );
    }

    #[test]
    fn class_insufficient_resources_is_911() {
        let c = PetSkillClassCtx {
            can_pay: false,
            ..cls_ctx()
        };
        assert_eq!(
            pet_skill_class_verdict(&c, Some(&cls_tpl()), Some(&cls_tpl())),
            PetSkillClassOutcome::Reject(RetCode::PetErrorSkillClassLimit)
        );
    }

    #[test]
    fn class_free_upgrade_ignores_can_pay() {
        let t = PetSkillClassTemplate {
            pet_num: 3,
            cost: None,
        };
        let c = PetSkillClassCtx {
            can_pay: false,
            ..cls_ctx()
        };
        assert!(matches!(
            pet_skill_class_verdict(&c, Some(&t), Some(&cls_tpl())),
            PetSkillClassOutcome::Accept { .. }
        ));
    }

    #[test]
    fn class_accept_carries_offset_and_cost() {
        match pet_skill_class_verdict(&cls_ctx(), Some(&cls_tpl()), Some(&cls_tpl())) {
            PetSkillClassOutcome::Accept {
                new_class,
                offset,
                cost,
            } => {
                assert_eq!(new_class, 2);
                assert_eq!(offset, 0);
                assert_eq!(cost, Some((1, 100)));
            }
            other => panic!("应通过，得到 {other:?}"),
        }
    }

    #[test]
    fn class_pet_count_boundary_passes() {
        // 正好等于 pet_num 也应通过（原版是 `<`）
        let c = PetSkillClassCtx {
            pet_count: 3,
            ..cls_ctx()
        };
        assert!(matches!(
            pet_skill_class_verdict(&c, Some(&cls_tpl()), Some(&cls_tpl())),
            PetSkillClassOutcome::Accept { .. }
        ));
    }
}
