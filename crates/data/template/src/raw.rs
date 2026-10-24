//! CSV 原始解析
//!
//! 实测 `data/apps/game_7010544/resource/template/csv/`：**2164 个 .csv**
//! （其中 445 个是模板表，其余为本地化/配置变体），编码 UTF-8 无 BOM，
//! 换行 **CRLF**，每个文件前 6 行是 schema：
//!
//! ```text
//! 1  AmberWelfareDB                    ← 实体类名（对应 bf.template.db.AmberWelfareDB）
//! 2  PK                                ← 主键标记
//! 3  3,3,3,3,3,3                       ← 列宽
//! 4  int,string,int,int,int,"list;,="  ← 类型 token
//! 5  id,cid,type,day,payValue,reward   ← 字段名
//! 6  ID,oppoCid,类型,签到天数,...        ← 中文注释（可忽略）
//! 7+ 1,1001,1,1,0,"1;2,3"              ← 数据
//! ```

/// 一张表的 schema。
#[derive(Debug, Clone)]
pub struct TableSchema {
    /// 实体类名（第 1 行）。
    pub entity: String,
    /// 是否单主键（第 2 行有 `PK`）。
    pub has_pk: bool,
    /// 字段名（第 5 行）。
    pub fields: Vec<String>,
    /// 字段类型（第 4 行）。
    pub types: Vec<FieldType>,
    /// 中文注释（第 6 行）。
    pub comments: Vec<String>,
}

/// 字段类型。
///
/// 445 个表实测只有这 6 种（外加 2 处 `sting` 笔误）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
    /// `int`
    Int,
    /// `long`
    Long,
    /// `string`
    Str,
    /// `bool`
    Bool,
    /// `double`
    Double,
    /// `list;<分隔符>,<键值符>`，例如 `list;,=` 表示 `;` 分隔、`=` 键值
    List {
        /// 元素分隔符。
        sep: char,
        /// 键值分隔符（无则 `None`）。
        kv: Option<char>,
    },
}

impl FieldType {
    /// 从 token 解析。
    ///
    /// `sting` / `Stirng` 等笔误按 `string` 处理（原 Java 侧也是这么容忍的）。
    #[must_use]
    pub fn parse(tok: &str) -> Self {
        let t = tok.trim().trim_matches('"');
        match t {
            "int" | "Int" => Self::Int,
            "long" | "Long" => Self::Long,
            "bool" | "boolean" | "Bool" => Self::Bool,
            "double" | "float" | "Double" => Self::Double,
            "sting" | "sring" | "string" | "String" => Self::Str,
            other if other.starts_with("list") => Self::parse_list(other),
            _ => Self::Str,
        }
    }

    /// 解析 `list` 系列 token（实测 8 种写法）。
    ///
    /// | token | sep | kv | 真实数据样例 |
    /// |---|---|---|---|
    /// | `list;`     | `;` | `None` | `1;2;3` |
    /// | `list;,=`   | `;` | `=`    | `1=10;2=20` |
    /// | `list;,`    | `;` | `None` | `1;2` |
    /// | `list=`     | `;` | `=`    | `261=120`（**无分隔符**，单键值） |
    /// | `list\|,;`  | `\|`| `;`    | `a\|b` |
    /// | `list\|`    | `\|`| `None` | |
    /// | `list#`     | `#` | `None` | |
    /// | `list`      | `;` | `None` | 默认 |
    ///
    /// 规则：`list` 之后**第一个字符就是 sep**，若紧跟 `,` 则跳过，
    /// 再下一个字符是 kv。唯一的例外是 `list=`——它没有 sep，只给了键值符，
    /// 按「sep 缺省 `;`、kv 为 `=`」处理（真表 `SeniorPetHaloUpgrade.cost`
    /// 存的就是 `261=120`）。
    fn parse_list(tok: &str) -> Self {
        let rest = &tok["list".len()..];
        // `list` / `list;`：无 kv
        if rest.is_empty() {
            return Self::List { sep: ';', kv: None };
        }
        // `list=`：只有键值符，没有分隔符
        if rest == "=" {
            return Self::List {
                sep: ';',
                kv: Some('='),
            };
        }
        let mut it = rest.chars();
        let sep = it.next().unwrap_or(';');
        let mut kv = it.next();
        if kv == Some(',') {
            kv = it.next();
        }
        // `list;,` 这类尾随逗号后为空的情况，kv 归 None
        if kv == Some(',') {
            kv = None;
        }
        Self::List { sep, kv }
    }

    /// 对应的 Rust 类型名（生成代码用）。
    #[must_use]
    pub fn rust_type(&self) -> &'static str {
        match self {
            Self::Int => "i32",
            Self::Long => "i64",
            Self::Str => "String",
            Self::Bool => "bool",
            Self::Double => "f64",
            Self::List { .. } => "Vec<String>",
        }
    }
}

/// 解析表头（**按单元格**，输入应已由 CSV reader 切好）。
///
/// 不要传「原始行字符串再用逗号切」——`list;,=` 这类 token 本身含逗号，
/// 但被双引号包裹，CSV reader 切完就是一个单元格；若先切再拼回字符串
/// 就会在逗号处断开。
///
/// # Errors
/// 行数不足 6，或字段数与类型数不一致。
pub fn parse_header_cells(head: &[Vec<String>]) -> Result<TableSchema, String> {
    if head.len() < 6 {
        return Err(format!("表头不足 6 行，实际 {}", head.len()));
    }
    let entity = head[0]
        .first()
        .map_or("", String::as_str)
        .trim()
        .to_string();
    let has_pk = head[1].first().is_some_and(|s| s.contains("PK"));

    // 尾部纯空单元格（类型行常写 `,,,` 占位）先剥掉再比长度。
    let trim_tail = |v: &Vec<String>| -> Vec<String> {
        let mut v = v.clone();
        while v.last().is_some_and(|s| s.trim().is_empty()) {
            v.pop();
        }
        v
    };
    let type_cells = trim_tail(&head[3]);
    let field_cells = trim_tail(&head[4]);
    let mut comment_cells = trim_tail(&head[5]);

    if field_cells.len() != type_cells.len() {
        return Err(format!(
            "{entity}: 字段数 {} 与类型数 {} 不一致",
            field_cells.len(),
            type_cells.len()
        ));
    }

    let types: Vec<FieldType> = type_cells.iter().map(|t| FieldType::parse(t)).collect();
    let fields: Vec<String> = field_cells.iter().map(|s| s.trim().to_string()).collect();
    comment_cells.resize(fields.len(), String::new());

    Ok(TableSchema {
        entity,
        has_pk,
        fields,
        types,
        comments: comment_cells,
    })
}

/// 解析表头（按原始 6 行字符串，供单测与简单场景使用）。
///
/// # Errors
/// 行数不足 6，或字段数与类型数不一致。
pub fn parse_header(lines: &[&str]) -> Result<TableSchema, String> {
    if lines.len() < 6 {
        return Err(format!("表头不足 6 行，实际 {}", lines.len()));
    }
    let entity = lines[0].trim().to_string();
    let has_pk = lines[1].contains("PK");
    let types: Vec<FieldType> = split_csv_row(lines[3])
        .iter()
        .map(|t| FieldType::parse(t))
        .collect();
    let fields = split_csv_row(lines[4]);
    let comments = split_csv_row(lines[5]);

    if fields.len() != types.len() {
        return Err(format!(
            "{entity}: 字段数 {} 与类型数 {} 不一致",
            fields.len(),
            types.len()
        ));
    }

    Ok(TableSchema {
        entity,
        has_pk,
        fields,
        types,
        comments,
    })
}

/// 按 CSV 规则切一行（处理双引号包裹）。
#[must_use]
pub fn split_csv_row(line: &str) -> Vec<String> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                if in_q && it.peek() == Some(&'"') {
                    cur.push('"');
                    it.next();
                } else {
                    in_q = !in_q;
                }
            }
            ',' if !in_q => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_header() {
        let lines = [
            "AmberWelfareDB",
            "PK",
            "3,3,3,3,3,3",
            "int,string,int,int,int,\"list;,=\"",
            "id,cid,type,day,payValue,reward",
            "ID,oppoCid,类型,签到天数,充值金额,奖励",
        ];
        let s = parse_header(&lines).expect("header");
        assert_eq!(s.entity, "AmberWelfareDB");
        assert!(s.has_pk);
        assert_eq!(s.fields.len(), 6);
        assert_eq!(s.types[0], FieldType::Int);
        assert_eq!(s.types[1], FieldType::Str);
        assert_eq!(
            s.types[5],
            FieldType::List {
                sep: ';',
                kv: Some('=')
            }
        );
    }

    #[test]
    fn tolerates_typo() {
        assert_eq!(FieldType::parse("sting"), FieldType::Str);
    }

    #[test]
    fn splits_quoted() {
        assert_eq!(
            split_csv_row("a,\"b,c\",d"),
            vec!["a".to_string(), "b,c".to_string(), "d".to_string()]
        );
    }

    /// CRLF 文件里，最后一列必须不带 `\r`，否则整型解析会失败。
    #[test]
    fn strips_cr_from_crlf_rows() {
        assert_eq!(
            split_csv_row("100,1,0,261=120,0\r"),
            vec![
                "100".to_string(),
                "1".to_string(),
                "0".to_string(),
                "261=120".to_string(),
                "0".to_string()
            ]
        );
    }

    /// 实测 8 种 list token 全覆盖（来自 445 张真表统计）。
    #[test]
    fn parses_all_real_list_tokens() {
        use FieldType::List;
        assert_eq!(FieldType::parse("list;"), List { sep: ';', kv: None });
        assert_eq!(
            FieldType::parse("list;,="),
            List {
                sep: ';',
                kv: Some('=')
            }
        );
        assert_eq!(FieldType::parse("list;,"), List { sep: ';', kv: None });
        assert_eq!(
            FieldType::parse("list="),
            List {
                sep: ';',
                kv: Some('=')
            }
        );
        assert_eq!(
            FieldType::parse("list|,;"),
            List {
                sep: '|',
                kv: Some(';')
            }
        );
        assert_eq!(FieldType::parse("list|"), List { sep: '|', kv: None });
        assert_eq!(FieldType::parse("list#"), List { sep: '#', kv: None });
        assert_eq!(FieldType::parse("list"), List { sep: ';', kv: None });
    }

    /// 真文件样例：`SeniorPetHaloUpgrade` 的 `list=` 列存的是 `261=120`。
    #[test]
    fn parses_real_file_with_crlf_and_list_eq() {
        let head = [
            "SeniorPetHaloUpgradeDB",
            "PK",
            "3,3,3,3,3",
            "int,int,int,list=,int",
            "id,haloType,haloLevel,cost,allAddValue",
            "ID,光环ID,光环技能2等级,升级消耗,财力加成",
        ];
        let s = parse_header(&head).expect("header");
        assert_eq!(s.entity, "SeniorPetHaloUpgradeDB");
        assert_eq!(s.fields.len(), 5);
        assert_eq!(
            s.types[3],
            FieldType::List {
                sep: ';',
                kv: Some('=')
            }
        );
        assert_eq!(s.comments[4], "财力加成");
    }
}
