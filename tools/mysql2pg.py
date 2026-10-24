#!/usr/bin/env python3
"""把 Navicat 导出的 MySQL DDL 转成 PostgreSQL 迁移脚本。

只处理本项目实际遇到的那一份 `data/sql/platform.sql`（Navicat MySQL 5.7 dump），
不是通用转换器。遇到没见过的写法会**报错退出**而不是猜，避免静默生成错 schema。

用法：
    python3 tools/mysql2pg.py \
        --src ../data/sql/platform.sql \
        --schema crates/data/db/migrations/0002_platform.sql \
        --seed   crates/data/db/migrations/0003_platform_seed.sql

转换规则见 docs/PG-MIGRATION.md。
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass, field

# --------------------------------------------------------------------------
# 类型映射
# --------------------------------------------------------------------------

#: MySQL 整数类型 → PostgreSQL。tinyint 一律升成 smallint：
#: 原库大量用 tinyint(1) 当布尔、tinyint(4) 当枚举，映射成 boolean 会改变语义。
INT_TYPES = {
    "tinyint": "smallint",
    "smallint": "smallint",
    "mediumint": "integer",
    "int": "integer",
    "integer": "integer",
    "bigint": "bigint",
}

TEXT_TYPES = {"text", "tinytext", "mediumtext", "longtext", "json"}

#: 定长/变长字符。json 单独处理成 text（原库没有真正的 json 列）。
CHAR_TYPES = {"char", "varchar"}

OTHER_TYPES = {
    "date": "date",
    "datetime": "timestamp",
    "timestamp": "timestamptz",
    "time": "time",
    "double": "double precision",
    "blob": "bytea",
    "tinyblob": "bytea",
    "mediumblob": "bytea",
    "longblob": "bytea",
    "binary": "bytea",
    "varbinary": "bytea",
}

#: 标识符里合法字符（反引号内）。
IDENT = re.compile(r"`([^`]+)`")


class ConvertError(Exception):
    """遇到不支持的写法。"""


@dataclass
class Column:
    name: str
    raw_type: str
    not_null: bool
    auto_inc: bool
    default: str | None
    comment: str | None


@dataclass
class Index:
    name: str
    columns: list[str]
    unique: bool
    fulltext: bool = False


@dataclass
class Table:
    name: str
    columns: list[Column] = field(default_factory=list)
    primary: list[str] = field(default_factory=list)
    indexes: list[Index] = field(default_factory=list)
    comment: str | None = None
    auto_increment_start: int | None = None


# --------------------------------------------------------------------------
# 词法：MySQL 字符串字面量
# --------------------------------------------------------------------------

#: MySQL 反斜杠转义表。注意 `\%` `\_` 在 MySQL 里是*保留反斜杠*的，
#: 但作为普通字符串处理时按字面保留即可，这里不特殊处理。
MYSQL_ESCAPES = {
    "0": "\0",
    "'": "'",
    '"': '"',
    "b": "\b",
    "n": "\n",
    "r": "\r",
    "t": "\t",
    "Z": "\x1a",
    "\\": "\\",
}


def mysql_unquote(s: str) -> str:
    """把 MySQL 单引号字符串内部文本解码成真实字符串。"""
    out: list[str] = []
    i = 0
    while i < len(s):
        ch = s[i]
        if ch == "\\" and i + 1 < len(s):
            nxt = s[i + 1]
            out.append(MYSQL_ESCAPES.get(nxt, nxt))
            i += 2
        else:
            out.append(ch)
            i += 1
    return "".join(out)


def pg_quote(value: str) -> str:
    """把真实字符串编码成 PostgreSQL 字面量（standard_conforming_strings=on）。"""
    return "'" + value.replace("'", "''") + "'"


def split_mysql_tuple(body: str) -> list[str]:
    """切分 `VALUES (...)` 括号内的字段，尊重引号与嵌套。"""
    out: list[str] = []
    cur: list[str] = []
    i = 0
    in_str = False
    depth = 0
    while i < len(body):
        ch = body[i]
        if in_str:
            if ch == "\\" and i + 1 < len(body):
                cur.append(ch)
                cur.append(body[i + 1])
                i += 2
                continue
            if ch == "'":
                in_str = False
            cur.append(ch)
        elif ch == "'":
            in_str = True
            cur.append(ch)
        elif ch == "(":
            depth += 1
            cur.append(ch)
        elif ch == ")":
            depth -= 1
            cur.append(ch)
        elif ch == "," and depth == 0:
            out.append("".join(cur).strip())
            cur = []
        else:
            cur.append(ch)
        i += 1
    tail = "".join(cur).strip()
    if tail:
        out.append(tail)
    return out


def convert_value(raw: str) -> str:
    """把一个 MySQL 值字面量转成 PostgreSQL 字面量。"""
    if raw.upper() == "NULL":
        return "NULL"
    if raw.startswith("'") and raw.endswith("'"):
        return pg_quote(mysql_unquote(raw[1:-1]))
    # 0x 十六进制 blob
    if raw.lower().startswith("0x"):
        return "'\\x" + raw[2:] + "'::bytea"
    # 数字（含负数、小数、科学计数）
    if re.fullmatch(r"-?\d+(\.\d+)?([eE][-+]?\d+)?", raw):
        return raw
    raise ConvertError(f"无法识别的值: {raw[:60]}")


# --------------------------------------------------------------------------
# DDL 解析
# --------------------------------------------------------------------------

CREATE_RE = re.compile(r"^CREATE TABLE `(?P<name>[^`]+)` \($")


def map_type(raw: str) -> tuple[str, str | None]:
    """MySQL 类型 → (PostgreSQL 类型, 附注)。

    `bigint(20) unsigned` 超出 PG bigint 范围，升成 numeric(20)。
    """
    m = re.fullmatch(r"([a-z]+)\s*(?:\(([^)]*)\))?\s*(unsigned)?", raw.strip(), re.I)
    if not m:
        raise ConvertError(f"无法解析类型: {raw}")
    base = m.group(1).lower()
    args = m.group(2)
    unsigned = bool(m.group(3))

    if base in INT_TYPES:
        pg = INT_TYPES[base]
        if unsigned:
            if base == "bigint":
                return "numeric(20)", "unsigned bigint → numeric(20)（超出 int8）"
            if pg == "integer":
                return "bigint", None
            return pg, None
        return pg, None
    if base == "decimal" or base == "numeric":
        return f"numeric({args})" if args else "numeric", None
    if base == "float":
        # float(p,s) 在 MySQL 是定点近似；PG real 不接受 (p,s)
        return "real", None
    if base in TEXT_TYPES:
        return "text", None
    if base in CHAR_TYPES:
        if not args:
            raise ConvertError(f"{base} 缺少长度")
        return f"{base}({args})", None
    if base in OTHER_TYPES:
        return OTHER_TYPES[base], None
    raise ConvertError(f"未映射的类型: {raw}")


COMMENT_RE = re.compile(r"\s+COMMENT\s+('(?:[^'\\]|\\.|'')*')\s*$", re.S)


def take_comment(text: str) -> tuple[str, str | None]:
    """剥掉行尾 `COMMENT '...'`。"""
    m = COMMENT_RE.search(text)
    if not m:
        return text, None
    lit = m.group(1)
    return text[: m.start()], mysql_unquote(lit[1:-1])


def parse_column(line: str) -> Column:
    m = IDENT.match(line)
    if not m:
        raise ConvertError(f"列定义无法识别: {line[:80]}")
    name = m.group(1)
    rest, comment = take_comment(line[m.end():].strip())

    auto_inc = "AUTO_INCREMENT" in rest
    rest = rest.replace("AUTO_INCREMENT", "")

    not_null = False
    if "NOT NULL" in rest:
        not_null = True
        rest = rest.replace("NOT NULL", "")
    rest = rest.replace("NULL", "") if rest.strip().startswith("NULL") else rest

    default: str | None = None
    dm = re.search(r"\bDEFAULT\s+('(?:[^'\\]|\\.)*'|-?\d+(?:\.\d+)?|NULL|CURRENT_TIMESTAMP(?:\(\d*\))?)",
                   rest, re.I)
    if dm:
        default = dm.group(1)
        rest = rest[: dm.start()] + rest[dm.end():]

    rest = re.sub(r"\b(CHARACTER SET|COLLATE)\s+\S+", "", rest, flags=re.I)
    rest = rest.replace("ON UPDATE CURRENT_TIMESTAMP", "").strip().rstrip(",")
    rest = re.sub(r"\s+", " ", rest).strip()

    return Column(name=name, raw_type=rest, not_null=not_null, auto_inc=auto_inc,
                  default=default, comment=comment)


KEY_RE = re.compile(r"^(?P<kind>PRIMARY KEY|UNIQUE KEY|FULLTEXT KEY|KEY|INDEX)\s*"
                    r"(?:`(?P<name>[^`]+)`\s*)?\((?P<cols>[^)]*)\)", re.I)


def parse_key_column(raw: str) -> str:
    """`` `name`(191) `` → ``name``。

    前缀长度是 MySQL 给长文本建前缀索引用的，PG 的 btree 不支持，
    而且对 varchar(n) 来说整列索引本来就等价（n 很小）。
    """
    m = re.match(r"`([^`]+)`", raw.strip())
    if not m:
        raise ConvertError(f"索引列无法识别: {raw}")
    return m.group(1)


def parse_key(line: str) -> tuple[str, Index | None, list[str]]:
    """返回 (kind, index, primary_columns)。"""
    m = KEY_RE.match(line)
    if not m:
        raise ConvertError(f"索引无法识别: {line[:80]}")
    kind = m.group("kind").upper()
    cols = [parse_key_column(c) for c in split_mysql_tuple(m.group("cols"))]
    if kind == "PRIMARY KEY":
        return kind, None, cols
    idx = Index(name=m.group("name") or "_".join(cols), columns=cols,
                unique=kind == "UNIQUE KEY", fulltext=kind == "FULLTEXT KEY")
    return kind, idx, []


def parse_create(lines: list[str], start: int) -> tuple[Table, int]:
    m = CREATE_RE.match(lines[start].rstrip())
    if not m:
        raise ConvertError(f"建表头无法识别: {lines[start][:80]}")
    tb = Table(name=m.group("name"))
    i = start + 1
    while i < len(lines):
        raw = lines[i].rstrip()
        i += 1
        if raw.startswith(")"):
            fm = re.search(r"AUTO_INCREMENT=(\d+)", raw)
            if fm:
                tb.auto_increment_start = int(fm.group(1))
            cm = re.search(r"COMMENT=('(?:[^'\\]|\\.|'')*')", raw, re.S)
            if cm:
                tb.comment = mysql_unquote(cm.group(1)[1:-1])
            return tb, i
        line = raw.strip().rstrip(",")
        if not line:
            continue
        if line.startswith("`"):
            tb.columns.append(parse_column(line))
            continue
        kind, idx, primary = parse_key(line)
        if kind == "PRIMARY KEY":
            tb.primary = primary
        elif idx:
            tb.indexes.append(idx)
        else:
            raise ConvertError(f"未处理的键: {line[:80]}")
    raise ConvertError(f"建表 {tb.name} 未闭合")


# --------------------------------------------------------------------------
# 输出 SQL
# --------------------------------------------------------------------------

def q(ident: str) -> str:
    """引用标识符。"""
    return '"' + ident.replace('"', '""') + '"'


def render_column(c: Column, stats: dict[str, int]) -> str:
    pg, note = map_type(c.raw_type)
    if note:
        stats.setdefault("notes", 0)
        stats["notes"] += 1
        print(f"  注意: {c.name} {note}", file=sys.stderr)
    if c.auto_inc and pg not in ("smallint", "integer", "bigint"):
        # PG 的 identity 只支持这三种整型；unsigned bigint 被映射成 numeric(20)，
        # 这里降回 bigint（自增列的实际取值远小于 int8 上限）。
        print(f"  自增列 {c.name}: {pg} → bigint（identity 限制）", file=sys.stderr)
        pg = "bigint"
    parts = [q(c.name), pg]
    if c.auto_inc:
        parts.append("GENERATED BY DEFAULT AS IDENTITY")
    if c.not_null and not c.auto_inc:
        parts.append("NOT NULL")
    if c.default is not None and not c.auto_inc:
        d = c.default
        if d.upper() == "NULL":
            parts.append("DEFAULT NULL")
        elif d.startswith("'"):
            parts.append("DEFAULT " + pg_quote(mysql_unquote(d[1:-1])))
        elif d.upper().startswith("CURRENT_TIMESTAMP"):
            parts.append("DEFAULT CURRENT_TIMESTAMP")
        else:
            parts.append("DEFAULT " + d)
    return " ".join(parts)


def render_table(tb: Table, stats: dict[str, int]) -> list[str]:
    out: list[str] = []
    out.append(f"-- {tb.name}" + (f"  {tb.comment}" if tb.comment else ""))
    cols = [render_column(c, stats) for c in tb.columns]
    if tb.primary:
        cols.append("PRIMARY KEY (" + ", ".join(q(c) for c in tb.primary) + ")")
    body = ",\n    ".join(cols)
    out.append(f"CREATE TABLE IF NOT EXISTS {q(tb.name)} (\n    {body}\n);")

    if tb.comment:
        out.append(f"COMMENT ON TABLE {q(tb.name)} IS {pg_quote(tb.comment)};")
    for c in tb.columns:
        if c.comment:
            out.append(f"COMMENT ON COLUMN {q(tb.name)}.{q(c.name)} IS {pg_quote(c.comment)};")

    for idx in tb.indexes:
        if idx.fulltext:
            # PG 全文索引要建 GIN 表达式索引；原库的 FULLTEXT 只用于后台模糊搜索，
            # 这里降级成普通 btree，够用且不引入 extension。
            stats["fulltext"] += 1
            uniq = "CREATE INDEX"
            suffix = "   -- 原 FULLTEXT 降级为 btree"
        else:
            uniq = "CREATE UNIQUE INDEX" if idx.unique else "CREATE INDEX"
            suffix = ""
        # PG 的索引名是 schema 级唯一，MySQL 是表级；必须加表名前缀防撞名。
        name = f"{tb.name}_{idx.name}"
        cols_sql = ", ".join(q(c) for c in idx.columns)
        out.append(f'{uniq} IF NOT EXISTS {q(name)} ON {q(tb.name)} ({cols_sql});{suffix}')
    out.append("")
    return out


# --------------------------------------------------------------------------
# 主流程
# --------------------------------------------------------------------------

def convert(src: str) -> tuple[list[Table], list[tuple[str, list[str]]], dict[str, int]]:
    lines = src.splitlines()
    tables: list[Table] = []
    inserts: list[tuple[str, list[str]]] = []
    stats = {"fulltext": 0, "notes": 0, "dup_table": 0, "insert": 0}
    seen: set[str] = set()

    i = 0
    while i < len(lines):
        line = lines[i]
        if line.startswith("CREATE TABLE"):
            tb, i = parse_create(lines, i)
            if tb.name in seen:
                stats["dup_table"] += 1
                print(f"  跳过重复建表: {tb.name}", file=sys.stderr)
                continue
            seen.add(tb.name)
            tables.append(tb)
            continue
        if line.startswith("INSERT INTO"):
            m = re.match(r"INSERT INTO `([^`]+)` VALUES \((.*)\);\s*$", line)
            if not m:
                raise ConvertError(f"INSERT 无法识别: {line[:100]}")
            values = [convert_value(v) for v in split_mysql_tuple(m.group(2))]
            inserts.append((m.group(1), values))
            stats["insert"] += 1
        i += 1
    return tables, inserts, stats



#: 种子数据带显式 id，identity 序列不会自动推进，插入后按当前最大值校正。
#: 放在 seed 末尾而不是 schema 末尾 —— 顺序错了新插入就会和种子 id 撞车。
SEQUENCE_DO = [
    "-- 种子数据带显式 id，identity 序列不会自动推进，这里按当前最大值校正。",
    "DO $$",
    "DECLARE r record;",
    "        mx bigint;",
    "BEGIN",
    "    FOR r IN",
    "        SELECT c.table_name, c.column_name",
    "        FROM information_schema.columns c",
    "        JOIN information_schema.tables t",
    "          ON t.table_schema = c.table_schema AND t.table_name = c.table_name",
    "        WHERE c.table_schema = 'public'",
    "          AND c.is_identity = 'YES'",
    "          AND t.table_type = 'BASE TABLE'",
    "    LOOP",
    "        EXECUTE format('SELECT COALESCE(max(%I), 0) FROM %I',",
    "                       r.column_name, r.table_name) INTO mx;",
    "        IF mx > 0 THEN",
    "            PERFORM setval(pg_get_serial_sequence(r.table_name, r.column_name), mx);",
    "        END IF;",
    "    END LOOP;",
    "END $$;",
    "",
]

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", required=True)
    ap.add_argument("--schema", required=True)
    ap.add_argument("--seed", required=True)
    args = ap.parse_args()

    src = open(args.src, encoding="utf-8").read()
    tables, inserts, stats = convert(src)

    # ---- schema ----
    head = [
        "-- 由 tools/mysql2pg.py 从 data/sql/platform.sql 生成，不要手改。",
        f"-- 表数: {len(tables)}",
        "-- 方言差异说明见 docs/PG-MIGRATION.md",
        "",
        "SET client_min_messages = warning;",
        "",
    ]
    body: list[str] = []
    for tb in tables:
        body += render_table(tb, stats)

    # 自增序列对齐：种子数据插入的是显式 id，identity 序列不会自动推进。
    tail: list[str] = []
    open(args.schema, "w", encoding="utf-8").write("\n".join(head + body + tail))

    # ---- seed ----
    seed = [
        "-- 由 tools/mysql2pg.py 从 data/sql/platform.sql 生成，不要手改。",
        f"-- 行数: {len(inserts)}",
        "-- ON CONFLICT DO NOTHING: 迁移可重复执行。",
        "",
    ]
    seed += [f"INSERT INTO {q(t)} VALUES ({', '.join(v)}) ON CONFLICT DO NOTHING;"
             for t, v in inserts]
    seed.append("")
    # 自增序列必须放在数据之后校正，否则新插入会和种子 id 撞车。
    seed += SEQUENCE_DO
    open(args.seed, "w", encoding="utf-8").write("\n".join(seed))

    print(f"表 {len(tables)} / 行 {len(inserts)} / "
          f"FULLTEXT 降级 {stats['fulltext']} / 重复表 {stats['dup_table']}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except ConvertError as exc:
        print(f"转换失败: {exc}", file=sys.stderr)
        sys.exit(1)
