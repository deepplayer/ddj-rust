//! 命令行定义
//!
//! 设计取向：**一个二进制管全部服务**，靠子命令区分「跑什么」与「怎么管」。
//!
//! - `run` / `start` 是「跑服务」：前台阻塞 vs 后台守护
//! - `stop` / `status` / `list` / `attach` / `logs` 是「管服务」：全部走控制 socket

use clap::{Parser, Subcommand};

/// 我是大东家 服务端。
#[derive(Debug, Parser)]
#[command(name = "ddj", version, about = "我是大东家 服务端（Rust 重构版）")]
pub struct Cli {
    /// 配置目录。
    #[arg(
        short = 'c',
        long,
        global = true,
        default_value = "config",
        env = "DDJ_CONFIG_DIR"
    )]
    pub config_dir: String,

    /// 运行目录（pid / socket / 日志）。默认 `$XDG_RUNTIME_DIR/ddj` 或 `/tmp/ddj-<uid>`。
    #[arg(long, global = true, env = "DDJ_RUN_DIR")]
    pub run_dir: Option<String>,

    /// 异步工作线程数，0 = 按 CPU 核数。
    #[arg(short = 'w', long, global = true, default_value_t = 0)]
    pub workers: usize,

    /// 子命令。
    #[command(subcommand)]
    pub cmd: Cmd,
}

/// 子命令。
#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// 前台运行（容器 / systemd 用；关闭终端即退出）。
    Run {
        /// 服务名：platform / game / cross / hall / battle / world / gm。
        service: String,
    },

    /// 后台守护运行（脱离 SSH 会话，断开不影响）。
    Start {
        /// 服务名。
        service: String,
    },

    /// 停止后台实例。
    Stop {
        /// 服务名。
        service: String,
    },

    /// 重启后台实例。
    Restart {
        /// 服务名。
        service: String,
    },

    /// 查看运行状态（省略服务名 = 全部）。
    Status {
        /// 服务名。
        service: Option<String>,
    },

    /// 列出全部服务与状态。
    List,

    /// 连上正在运行的实例看实时输出（Ctrl-C 只断开，不停止服务）。
    Attach {
        /// 服务名。
        service: String,
        /// 连接时回放的最后行数。
        #[arg(short, long, default_value_t = 50)]
        tail: usize,
    },

    /// 打印日志文件。
    Logs {
        /// 服务名。
        service: String,
        /// 跟随输出。
        #[arg(short, long)]
        follow: bool,
        /// 显示最后 N 行。
        #[arg(short = 'n', long, default_value_t = 200)]
        lines: usize,
    },

    /// 列出可用的服务名。
    Services,
}
