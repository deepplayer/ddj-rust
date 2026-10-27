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

    #[test]
    fn set_zero_unmounts_without_ownership() {
        // skinId == 0 是卸下，原版跳过 existedSkin 检查
        assert_eq!(
            set_skin_verdict(Some(&pet()), 0),
            SkinOutcome::Accept(SkinEffect::Set { skin_id: 0 })
        );
    }
}
