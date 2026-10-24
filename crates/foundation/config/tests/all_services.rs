//! 校验 `config/*.toml` 全部能按真实 schema 加载。
//!
//! `#[serde(deny_unknown_fields)]` 会让拼错的键直接报错，
//! 所以这个测试等价于「配置与结构体没有脱节」。

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use ddj_config::{ConfigLoader, Environment};

const SERVICES: &[&str] = &["platform", "game", "cross", "hall", "battle", "world", "gm"];

#[test]
fn every_service_config_loads() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../config");
    if !std::path::Path::new(dir).is_dir() {
        eprintln!("跳过：找不到 config 目录 {dir}");
        return;
    }

    let mut ok = 0;
    for svc in SERVICES {
        let cfg = ConfigLoader::new(dir, *svc)
            .with_env(Environment::Dev)
            .load()
            .unwrap_or_else(|e| panic!("{svc}.toml 加载失败: {e}"));

        assert_eq!(&cfg.service.name, svc, "服务名不符");
        assert_ne!(cfg.net.tcp_listen.port(), 0, "{svc} 端口为 0");
        assert!(!cfg.db.platform_url.is_empty());
        println!(
            "{svc:9} tcp={} http={} rpc={:?}",
            cfg.net.tcp_listen, cfg.net.http_listen, cfg.net.rpc_listen
        );
        ok += 1;
    }
    assert_eq!(ok, SERVICES.len());
}
