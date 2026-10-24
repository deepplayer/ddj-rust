//! 背包推送包（11004 / 11007）
//!
//! 对应 Java：
//! - `ItemServerModule.sendItemUpdate(String playerId, List<Item>)` → `ItemUpdateResp_11004`（66 B）
//! - `ItemServerModule.sendItemEffectiveUpdate(String, List<ItemEffective>)` → `ItemEffectiveUpdateResp_11007`（66 B）
//!
//! 两者的共同点：**列表为空时直接 `return`，不发包**（字节码偏移 0-9）。
//! 推送字段只含客户端渲染所需的最小集合，其余状态留在服务端。

use ddj_pb::gen::item_message::{
    ItemEffectiveUpdateResp11007, ItemUpdateResp11004, MsgItemData, MsgItemEffectiveData,
};

/// `bf.game.server.module.item.Item` 的推送子集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BagItem {
    /// `templateId`。
    pub template_id: i32,
    /// `num`。
    pub num: i64,
}

/// `bf.game.server.module.item.ItemEffective` 的推送子集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BagEffectiveItem {
    /// `dbId`（唯一 id）。
    pub id: i64,
    /// `templateId`。
    pub template_id: i32,
    /// `num`。
    pub num: i64,
    /// `deadline`（毫秒）。
    pub deadline: i64,
}

/// 构造 `ItemUpdateResp_11004`。
///
/// 与 `Item.toMsgItemDataBuilder()` 字段映射一致：只有 `templateId` + `num`。
/// 空列表返回 `None`（原版直接 return，不发包）。
#[must_use]
pub fn build_item_update(items: &[BagItem]) -> Option<ItemUpdateResp11004> {
    if items.is_empty() {
        return None;
    }
    Some(ItemUpdateResp11004 {
        msg_item_data: items
            .iter()
            .map(|it| MsgItemData {
                template_id: Some(it.template_id),
                num: Some(it.num),
            })
            .collect(),
    })
}

/// 构造 `ItemEffectiveUpdateResp_11007`。
///
/// 与 `ItemEffective.toMsgItemEffective()` 字段映射一致：`dbId` + `templateId` + `num` + `deadline`。
/// 空列表返回 `None`。
#[must_use]
pub fn build_effective_update(items: &[BagEffectiveItem]) -> Option<ItemEffectiveUpdateResp11007> {
    if items.is_empty() {
        return None;
    }
    Some(ItemEffectiveUpdateResp11007 {
        msg_item_effective_data: items
            .iter()
            .map(|it| MsgItemEffectiveData {
                id: Some(it.id),
                template_id: Some(it.template_id),
                num: Some(it.num),
                deadline: Some(it.deadline),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_list_sends_nothing() {
        assert!(build_item_update(&[]).is_none());
        assert!(build_effective_update(&[]).is_none());
    }

    #[test]
    fn item_update_maps_template_and_num_only() {
        let msg = build_item_update(&[BagItem {
            template_id: 11002,
            num: 7,
        }])
        .expect("应发包");
        assert_eq!(msg.msg_item_data.len(), 1);
        let d = &msg.msg_item_data[0];
        assert_eq!(d.template_id, Some(11002));
        assert_eq!(d.num, Some(7));
    }

    #[test]
    fn item_update_preserves_order_and_count() {
        let items: Vec<BagItem> = (0..5)
            .map(|i| BagItem {
                template_id: 1000 + i,
                num: i64::from(i),
            })
            .collect();
        let msg = build_item_update(&items).expect("应发包");
        assert_eq!(msg.msg_item_data.len(), 5);
        for (i, d) in msg.msg_item_data.iter().enumerate() {
            assert_eq!(d.template_id, Some(1000 + i as i32));
            assert_eq!(d.num, Some(i as i64));
        }
    }

    #[test]
    fn effective_update_carries_deadline() {
        let msg = build_effective_update(&[BagEffectiveItem {
            id: 88_001,
            template_id: 8001,
            num: 3,
            deadline: 1_700_000_000_000,
        }])
        .expect("应发包");
        let d = &msg.msg_item_effective_data[0];
        assert_eq!(d.id, Some(88_001));
        assert_eq!(d.template_id, Some(8001));
        assert_eq!(d.num, Some(3));
        assert_eq!(d.deadline, Some(1_700_000_000_000));
    }

    #[test]
    fn both_push_messages_roundtrip_through_protobuf() {
        use prost::Message as _;
        let msg = build_item_update(&[BagItem {
            template_id: 5,
            num: 1,
        }])
        .expect("应发包");
        let bytes = msg.encode_to_vec();
        let back = ItemUpdateResp11004::decode(&bytes[..]).expect("应能解码");
        assert_eq!(back, msg);
    }
}
