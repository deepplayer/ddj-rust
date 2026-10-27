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
