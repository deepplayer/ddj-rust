# ADR-0003：CSV 作为配置表唯一真相源

- 状态：已接受
- 日期：2026-10-24

## 背景

原系统配置表是 CSV，跑起来后转成 `static HashMap`，字段类型靠约定。
实测 `data/apps/game_7010544/resource/template/csv/`：**2164 个 .csv**，
其中 **445 张是模板表**（共 78215 行数据），UTF-8 无 BOM、**CRLF**、前 6 行是 schema。

## 决定

不手写结构体，从 CSV 头部生成 Rust struct：

```text
第 1 行  实体名        SeniorPetHaloUpgradeDB
第 2 行  PK
第 3 行  列宽          3,3,3,3,3
第 4 行  类型          int,int,int,list=,int
第 5 行  字段名        id,haloType,haloLevel,cost,allAddValue
第 6 行  中文注释      ID,光环ID,光环技能2等级,升级消耗,财力加成
```

`FieldType` 只支持实测出现的 6 种（`int`/`long`/`string`/`bool`/`double`/`list`），
`list` 有 8 种写法，已全部覆盖并有单测。

## 后果

- 好的：445 张表全部能解析（实测 0 警告），字段改名会在编译期报错
- 坏的：CSV 里的笔误（`sting`）必须容忍，不能报错
- 注意：**必须用 `csv` crate 按单元格解析**。真表里有带引号的多行单元格，
  按 `str::lines()` 切会切碎；`list;,=` 含逗号，先 join 再 split 也会断错。
