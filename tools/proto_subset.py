#!/usr/bin/env python3
"""从 `work/letsproto.proto` 里裁出「某个功能链路」需要的消息子集。

背景：全量 117 个顶层块 / 60403 行，prost 生成物 4.4 MB，
rustc 峰值内存 ~900 MB。开发机上没法每次都编译它。

做法：按顶层块切分（一条块内部是一个功能域的全部嵌套 message），
从种子消息出发做依赖闭包，只输出被用到的块，保持嵌套结构不变
（因为消息之间用 `PlayerMessage.MsgGmLanConfig` 这种嵌套引用）。

用法：
    python3 tools/proto_subset.py --seed LoginReq_4,LoginResp_4 \\
        --out crates/protocol/pb/proto/login.proto
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass, field

TOP_RE = re.compile(r"^message\s+(\w+)\s*\{")
NESTED_RE = re.compile(r"^\s*message\s+(\w+)\s*\{")
# 字段声明：`optional PlayerMessage.MsgGmLanConfig lanConfig = 10;`
FIELD_RE = re.compile(r"^\s*(?:optional|required|repeated)\s+([A-Za-z_][\w.]*)\s+\w+\s*=\s*\d+")
SCALARS = {
    "double", "float", "int32", "int64", "uint32", "uint64", "sint32", "sint64",
    "fixed32", "fixed64", "sfixed32", "sfixed64", "bool", "string", "bytes",
}


@dataclass
class Block:
    """一个顶层 message 块（含全部嵌套消息）。"""

    name: str
    start: int
    end: int
    nested: set[str] = field(default_factory=set)

    @property
    def lines(self) -> int:
        """块行数。"""
        return self.end - self.start


def split_blocks(lines: list[str]) -> list[Block]:
    """按顶层 `message X {` 切块，用去注释后的括号计数找闭合。"""
    blocks: list[Block] = []
    i = 0
    while i < len(lines):
        m = TOP_RE.match(lines[i])
        if not m:
            i += 1
            continue
        start = i
        depth = 0
        while i < len(lines):
            bare = lines[i].split("//")[0]
            depth += bare.count("{") - bare.count("}")
            i += 1
            if depth == 0:
                break
        blk = Block(name=m.group(1), start=start, end=i)
        for j in range(start + 1, i):
            nm = NESTED_RE.match(lines[j])
            if nm:
                blk.nested.add(nm.group(1))
        blocks.append(blk)
    return blocks


def block_refs(lines: list[str], blk: Block) -> set[str]:
    """块内所有字段声明的类型名（已展开 `A.B` → A 与 B）。"""
    out: set[str] = set()
    for j in range(blk.start, blk.end):
        fm = FIELD_RE.match(lines[j])
        if not fm:
            continue
        tname = fm.group(1)
        if tname in SCALARS:
            continue
        if "." in tname:
            head, tail = tname.split(".", 1)
            out.add(head)
            out.add(tail)
        else:
            out.add(tname)
    return out


def resolve(blocks: list[Block]) -> tuple[dict[str, Block], dict[str, Block]]:
    """建立 `顶层名 → 块` 与 `嵌套名 → 块` 两张索引。"""
    by_top = {b.name: b for b in blocks}
    by_nested: dict[str, Block] = {}
    for b in blocks:
        for n in b.nested:
            by_nested.setdefault(n, b)
    return by_top, by_nested


def closure(seeds: list[str], by_top: dict[str, Block], by_nested: dict[str, Block],
            lines: list[str]) -> set[str]:
    """从种子出发求顶层块闭包。"""
    picked: set[str] = set()
    queue: list[str] = []
    for s in seeds:
        if s in by_top:
            queue.append(s)
        elif s in by_nested:
            queue.append(by_nested[s].name)
        else:
            raise SystemExit(f"种子消息不存在: {s}")

    while queue:
        name = queue.pop()
        if name in picked:
            continue
        picked.add(name)
        blk = by_top[name]
        for ref in block_refs(lines, blk):
            if ref in picked:
                continue
            if ref in by_top:
                queue.append(ref)
            elif ref in by_nested:
                queue.append(by_nested[ref].name)
            elif ref not in SCALARS:
                print(f"  警告: {name} 引用了未知类型 {ref}", file=sys.stderr)
    return picked


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", default="../work/letsproto.proto")
    ap.add_argument("--out", required=True)
    ap.add_argument("--seed", required=True, help="逗号分隔的种子消息名")
    args = ap.parse_args()

    lines = open(args.src, encoding="utf-8").read().splitlines()
    blocks = split_blocks(lines)
    by_top, by_nested = resolve(blocks)
    picked = closure([s.strip() for s in args.seed.split(",") if s.strip()],
                     by_top, by_nested, lines)

    header = [
        'syntax = "proto2";',
        "package pb;",
        "",
        f"// 由 tools/proto_subset.py 生成，不要手改。",
        f"// 源: {args.src}（全量 {len(blocks)} 块 / {len(lines)} 行）",
        f"// 种子: {args.seed}",
        f"// 闭包: {len(picked)} 块",
        "",
    ]
    body: list[str] = []
    for blk in sorted(blocks, key=lambda b: b.start):
        if blk.name in picked:
            body += lines[blk.start:blk.end]
            body.append("")
    open(args.out, "w", encoding="utf-8").write("\n".join(header + body))

    total = sum(by_top[n].lines for n in picked)
    print(f"选中 {len(picked)} 块 / {total} 行（全量 {len(blocks)} 块 / {len(lines)} 行）")
    print("块: " + ", ".join(sorted(picked)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
