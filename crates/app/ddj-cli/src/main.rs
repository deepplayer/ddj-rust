//! # ddj — 单可执行入口
//!
//! 7 个服务合并成一个二进制，靠子命令区分。核心设计取舍：
//!
//! | 需求 | 做法 |
//! |---|---|
//! | 不受 shell 约束 | `start` 时 `fork` + `setsid`，SIGHUP 忽略 |
//! | 换终端还能接回来 | 固定路径 Unix socket，任何进程都能连 |
//! | 能看实时输出 | `attach` 走 socket 订阅内存环形缓冲 |
//! | 容器里也能用 | `run` 前台模式，交给 systemd / docker 托管 |
//!
//! 顺序上有个硬约束：`fork` 必须在创建 tokio runtime 之前。
//! 多线程进程 fork 后子进程只剩调用线程，别的线程持有的锁会永久不可用。

// 可执行 crate 没有对外 API，`unreachable_pub` 在此无意义。
#![allow(unreachable_pub)]
// 测试里 expect/panic 是断言手段，不属于生产路径。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod cli;
mod cmd;
mod console;
mod control;
mod daemon;

use clap::Parser;
use cli::{Cli, Cmd};
use daemon::Paths;
use std::process::ExitCode;

fn build_rt(workers: usize) -> anyhow::Result<tokio::runtime::Runtime> {
    let mut b = tokio::runtime::Builder::new_multi_thread();
    b.enable_all();
    if workers > 0 {
        b.worker_threads(workers);
    }
    Ok(b.build()?)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let paths = Paths::resolve(cli.run_dir.as_deref());

    match cli.cmd {
        Cmd::Services => {
            cmd::services();
            ExitCode::SUCCESS
        }

        // start / restart 必须先 fork 再建 runtime，所以不能塞进 block_on。
        Cmd::Start { service } => {
            match cmd::start(&service, &cli.config_dir, cli.workers, &paths) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail(e),
            }
        }

        Cmd::Restart { service } => {
            let rt = match build_rt(cli.workers) {
                Ok(r) => r,
                Err(e) => return fail(e),
            };
            if cmd::running_pid(&service, &paths).is_some() {
                if let Err(e) = rt.block_on(cmd::stop(&service, &paths)) {
                    return fail(e);
                }
            }
            // 前一个 runtime 必须先销毁，再 fork 出新进程。
            drop(rt);
            match cmd::start(&service, &cli.config_dir, cli.workers, &paths) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail(e),
            }
        }

        other => {
            let rt = match build_rt(cli.workers) {
                Ok(r) => r,
                Err(e) => return fail(e),
            };
            let res = rt.block_on(async move {
                match other {
                    Cmd::Run { service } => cmd::run(&service, &cli.config_dir).await,
                    Cmd::Stop { service } => cmd::stop(&service, &paths).await,
                    Cmd::Status { service } => match service {
                        Some(s) => cmd::status_one(&s, &paths).await,
                        None => cmd::list(&paths).await,
                    },
                    Cmd::List => cmd::list(&paths).await,
                    Cmd::Attach { service, tail } => cmd::attach(&service, tail, &paths).await,
                    Cmd::Logs {
                        service,
                        follow,
                        lines,
                    } => cmd::logs(&service, lines, follow, &paths).await,
                    Cmd::Services | Cmd::Start { .. } | Cmd::Restart { .. } => Ok(()),
                }
            });
            match res {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail(e),
            }
        }
    }
}

fn fail(e: anyhow::Error) -> ExitCode {
    eprintln!("错误: {e:#}");
    ExitCode::FAILURE
}
