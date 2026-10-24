//! 配置加载：默认值 → TOML → 环境变量

use crate::schema::Config;
use ddj_core::Result;
use std::path::Path;

/// 运行环境。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Environment {
    /// 本地开发。
    #[default]
    Dev,
    /// 预发布。
    Staging,
    /// 生产。
    Prod,
}

impl Environment {
    /// 从 `DDJ_ENV` 读取，缺省 `dev`。
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var("DDJ_ENV").as_deref() {
            Ok("prod" | "production") => Self::Prod,
            Ok("staging") => Self::Staging,
            _ => Self::Dev,
        }
    }
}

/// 配置加载器。
#[derive(Debug)]
pub struct ConfigLoader {
    dir: std::path::PathBuf,
    service: String,
    env: Environment,
}

impl ConfigLoader {
    /// 构造。
    #[must_use]
    pub fn new(dir: impl AsRef<Path>, service: impl Into<String>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
            service: service.into(),
            env: Environment::from_env(),
        }
    }

    /// 指定环境。
    #[must_use]
    pub fn with_env(mut self, env: Environment) -> Self {
        self.env = env;
        self
    }

    /// 加载。
    ///
    /// 顺序：内置默认 → `{service}.toml` → `{service}.{env}.toml`
    /// 环境变量覆盖在 [`crate::validate::Validate`] 之前做（见 `apply_env`）。
    pub fn load(&self) -> Result<Config> {
        let mut value = toml::Value::try_from(Config::default())
            .map_err(|e| ddj_core::AppError::BadRequest(format!("序列化默认配置: {e}")))?;

        let base = self.dir.join(format!("{}.toml", self.service));
        merge_file(&mut value, &base)?;

        let env_name = match self.env {
            Environment::Dev => "dev",
            Environment::Staging => "staging",
            Environment::Prod => "prod",
        };
        let overlay = self.dir.join(format!("{}.{env_name}.toml", self.service));
        merge_file(&mut value, &overlay)?;

        // 环境变量最后覆盖：容器/CI 里改一个值不必重打镜像。
        apply_env(&mut value)?;

        value
            .try_into()
            .map_err(|e| ddj_core::AppError::BadRequest(format!("合并配置: {e}")))
    }
}

/// TOML 深合并：`over` 里的 table 递归进去，其余值直接覆盖。
fn deep_merge(base: &mut toml::Value, over: toml::Value) {
    match (base, over) {
        (toml::Value::Table(b), toml::Value::Table(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(slot) => deep_merge(slot, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (slot, v) => *slot = v,
    }
}

/// 读一个 TOML 文件并深合并进 `value`。文件不存在时静默跳过。
fn merge_file(value: &mut toml::Value, path: &Path) -> Result<()> {
    if !path.exists() {
        tracing::debug!(path = %path.display(), "配置文件不存在，跳过");
        return Ok(());
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| ddj_core::AppError::BadRequest(format!("读配置 {}: {e}", path.display())))?;
    let file_val: toml::Value = toml::from_str(&text)
        .map_err(|e| ddj_core::AppError::BadRequest(format!("解析 {}: {e}", path.display())))?;
    deep_merge(value, file_val);
    tracing::debug!(path = %path.display(), "配置已合并");
    Ok(())
}

/// 环境变量前缀：`DDJ__DB__GAME_URL` -> `db.game_url`。
const ENV_PREFIX: &str = "DDJ__";

/// 把 `DDJ__<段>__<段>` 形式的环境变量覆盖进配置树。
///
/// 值按 TOML 标量解析：`true`/`false` 为布尔，纯数字为整数，其余为字符串。
/// 解析失败直接报错而不是静默忽略，配置写错应该在启动时就炸。
fn apply_env(value: &mut toml::Value) -> Result<()> {
    let mut count = 0usize;
    for (key, raw) in std::env::vars() {
        let Some(rest) = key.strip_prefix(ENV_PREFIX) else {
            continue;
        };
        let segs: Vec<String> = rest
            .split("__")
            .filter(|s| !s.is_empty())
            .map(str::to_ascii_lowercase)
            .collect();
        if segs.is_empty() {
            continue;
        }
        set_path(value, &segs, parse_scalar(&raw)).map_err(|e| {
            ddj_core::AppError::BadRequest(format!("环境变量 {key} 覆盖配置失败: {e}"))
        })?;
        count += 1;
    }
    if count > 0 {
        tracing::info!(count, "环境变量已覆盖配置");
    }
    Ok(())
}

/// 把值写到 `segs` 指定的路径，中途缺的 table 自动建。
fn set_path(
    root: &mut toml::Value,
    segs: &[String],
    v: toml::Value,
) -> std::result::Result<(), String> {
    let (last, parents) = segs.split_last().ok_or("空路径")?;
    let mut cur = root;
    for s in parents {
        if !cur.is_table() {
            return Err(format!("{s} 的父节点不是 table"));
        }
        let tbl = cur.as_table_mut().ok_or("内部错误")?;
        cur = tbl
            .entry(s.clone())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    }
    cur.as_table_mut()
        .ok_or("末级父节点不是 table")?
        .insert(last.clone(), v);
    Ok(())
}

/// 按 TOML 标量解析环境变量原值。
fn parse_scalar(raw: &str) -> toml::Value {
    match raw {
        "true" => return toml::Value::Boolean(true),
        "false" => return toml::Value::Boolean(false),
        _ => {}
    }
    if let Ok(i) = raw.parse::<i64>() {
        return toml::Value::Integer(i);
    }
    if let Ok(f) = raw.parse::<f64>() {
        return toml::Value::Float(f);
    }
    toml::Value::String(raw.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实配置片段：`[net]` 只写了 `tcp_listen`，其它键应保留默认值。
    #[test]
    fn deep_merge_keeps_defaults() {
        let default = Config::default();
        let default_port = default.net.tcp_listen.port();
        let default_max = default.net.max_conn_total;

        let dir = std::env::temp_dir().join("ddj-cfg-test");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("t.toml");
        std::fs::write(&f, "[net]\ntcp_listen = \"0.0.0.0:20544\"\n").unwrap();

        let mut value = toml::Value::try_from(Config::default()).unwrap();
        merge_file(&mut value, &f).unwrap();
        let cfg: Config = value.try_into().unwrap();

        assert_eq!(cfg.net.tcp_listen.port(), 20544, "文件里的值必须覆盖默认值");
        assert_ne!(default_port, 20544, "默认端口不应是 20544");
        assert_eq!(
            cfg.net.max_conn_total, default_max,
            "同段未写字段应保留默认值"
        );
    }

    /// 二级覆盖：base 一份、env overlay 一份，后者覆盖前者。
    #[test]
    fn overlay_overrides_base() {
        let dir = std::env::temp_dir().join("ddj-cfg-overlay");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("game.toml"),
            "[service]\nname = \"game\"\nserver_id = 1\n",
        )
        .unwrap();
        std::fs::write(dir.join("game.dev.toml"), "[service]\nserver_id = 99\n").unwrap();

        let cfg = ConfigLoader::new(&dir, "game")
            .with_env(Environment::Dev)
            .load()
            .unwrap();

        assert_eq!(cfg.service.name, "game");
        assert_eq!(cfg.service.server_id, 99, "overlay 必须覆盖 base");
    }
}
