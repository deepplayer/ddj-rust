//! 控制通道（Unix domain socket）
//!
//! 解决的正是「换一个终端会话还能接回来」：
//! 守护进程在固定路径监听一个 socket，任何后来者都能连上去查状态、
//! 拉实时输出、或者要求它退出——**不需要知道它当初从哪个 shell 启动**。
//!
//! ## 协议
//! 请求只有一行 ASCII，响应分两种：
//!
//! | 请求 | 响应 |
//! |---|---|
//! | `ping` | 一行 JSON `{"ok":true,...}` |
//! | `status` | 一行 JSON（含 uptime / online / handlers / templates） |
//! | `stop` | 一行 JSON，随后进程开始优雅退出 |
//! | `subscribe <n>` | 先回放最近 n 行，然后持续推送新行，直到连接关闭 |
//!
//! 用行协议而不是二进制：出问题能直接 `nc -U` 手工调，运维成本低。

use crate::console::Console;
use ddj_game_core::Stats;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;

/// 状态响应。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResp {
    /// 是否成功。
    pub ok: bool,
    /// 进程号。
    pub pid: u32,
    /// 运行指标。
    #[serde(flatten)]
    pub stats: Stats,
}

/// 服务端共享状态。
pub struct ControlState {
    /// 控制台。
    pub console: Console,
    /// 指标快照（由心跳任务刷新）。
    pub stats: Arc<RwLock<Stats>>,
    /// 停止信号。
    pub stop: watch::Sender<bool>,
    /// 进程号。
    pub pid: u32,
}

/// 启动控制服务。
///
/// # Errors
/// socket 绑定失败。
pub async fn serve(path: &Path, state: Arc<ControlState>) -> std::io::Result<()> {
    // 上一次非正常退出可能留下 socket 文件。
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "accept 失败");
                continue;
            }
        };
        let st = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle(stream, st).await {
                tracing::debug!(error = %e, "控制连接结束");
            }
        });
    }
}

async fn handle(stream: UnixStream, st: Arc<ControlState>) -> std::io::Result<()> {
    let (rd, mut wr) = stream.into_split();
    let mut rd = BufReader::new(rd);
    let mut line = String::new();
    if rd.read_line(&mut line).await? == 0 {
        return Ok(());
    }
    let cmd = line.trim();
    let (verb, arg) = match cmd.split_once(' ') {
        Some((a, b)) => (a, b.trim()),
        None => (cmd, ""),
    };

    match verb {
        "ping" => {
            let body = serde_json::to_string(&serde_json::json!({"ok": true, "pid": st.pid}))
                .unwrap_or_default();
            wr.write_all(format!("{body}\n").as_bytes()).await?;
        }
        "status" => {
            let snap = {
                let g = st.stats.read().unwrap_or_else(|e| e.into_inner());
                g.clone()
            };
            let body = serde_json::to_string(&StatusResp {
                ok: true,
                pid: st.pid,
                stats: snap,
            })
            .unwrap_or_else(|_| r#"{"ok":false}"#.to_owned());
            wr.write_all(format!("{body}\n").as_bytes()).await?;
        }
        "stop" => {
            // 必须先发信号：客户端可能已断开，写响应失败时不能连带丢掉停止动作。
            let agreed = st.stop.send(true).is_ok();
            let body = if agreed {
                "{\"ok\":true}"
            } else {
                "{\"ok\":false,\"error\":\"无接收者\"}"
            };
            let _ = wr.write_all(format!("{body}\n").as_bytes()).await;
        }
        "subscribe" => {
            let n: usize = arg.parse().unwrap_or(50);
            // 先补上上下文，避免连上去一片空白不知道为什么。
            for l in st.console.tail(n) {
                wr.write_all(format!("{l}\n").as_bytes()).await?;
            }
            wr.flush().await?;
            let mut rx = st.console.subscribe();
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
            loop {
                tokio::select! {
                    got = rx.recv() => match got {
                        Ok(l) => {
                            if wr.write_all(format!("{l}\n").as_bytes()).await.is_err() {
                                break;
                            }
                        }
                        // 跟不上（客户端太慢）就跳过，不阻塞业务线程。
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::debug!(skipped = n, "attach 客户端落后");
                        }
                        Err(_) => break,
                    },
                    _ = tick.tick() => {
                        // 心跳：探测对端是否已断开，同时让终端看起来有活性。
                        if wr.write_all(b"").await.is_err() {
                            break;
                        }
                        wr.flush().await.ok();
                    }
                }
            }
        }
        other => {
            wr.write_all(format!("{{\"ok\":false,\"error\":\"未知命令 {other}\"}}\n").as_bytes())
                .await?;
        }
    }
    wr.flush().await.ok();
    Ok(())
}

/// 连上去要一行状态。
///
/// # Errors
/// 连接失败、超时、响应不是合法 JSON。
pub async fn status(path: &Path) -> std::io::Result<StatusResp> {
    let stream = UnixStream::connect(path).await?;
    let (rd, mut wr) = stream.into_split();
    wr.write_all(b"status\n").await?;
    let mut rd = BufReader::new(rd);
    let mut line = String::new();
    rd.read_line(&mut line).await?;
    serde_json::from_str(line.trim()).map_err(|e| std::io::Error::other(e.to_string()))
}

/// 要求对端退出。
///
/// # Errors
/// 连接或写入失败。
pub async fn stop(path: &Path) -> std::io::Result<()> {
    let stream = UnixStream::connect(path).await?;
    let (rd, mut wr) = stream.into_split();
    wr.write_all(b"stop\n").await?;
    wr.flush().await?;
    // 读回 ack：确认对端真的受理了，而不是连接上了却没处理。
    let mut rd = BufReader::new(rd);
    let mut line = String::new();
    tokio::time::timeout(std::time::Duration::from_secs(3), rd.read_line(&mut line))
        .await
        .map_err(|_| std::io::Error::other("等待停止确认超时"))??;
    if line.contains("\"ok\":true") {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "对端拒绝停止: {}",
            line.trim()
        )))
    }
}

/// 连上并持续打印输出，直到 `Ctrl-C` 或对端退出。
///
/// # Errors
/// 连接失败。
pub async fn attach(path: &Path, tail: usize) -> std::io::Result<()> {
    let stream = UnixStream::connect(path).await?;
    let (rd, mut wr) = stream.into_split();
    wr.write_all(format!("subscribe {tail}\n").as_bytes())
        .await?;
    wr.flush().await?;

    let mut rd = BufReader::new(rd);
    let mut buf = String::new();
    let mut out = tokio::io::stdout();
    loop {
        buf.clear();
        let n = rd.read_line(&mut buf).await?;
        if n == 0 {
            break;
        }
        out.write_all(buf.as_bytes()).await?;
        out.flush().await?;
    }
    Ok(())
}
