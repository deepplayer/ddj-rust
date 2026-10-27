//! 各子命令实现

use crate::console::Console;
use crate::control::{self, ControlState};
use crate::daemon::{self, Paths};
use anyhow::{bail, Context, Result};
use ddj_game_core::{Bootstrap, ServiceKind, Stats, StatsSource};
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// 解析服务名。
fn kind_of(name: &str) -> Result<ServiceKind> {
    ServiceKind::from_name(name).ok_or_else(|| {
        anyhow::anyhow!(
            "未知服务 {name}，可用：{}",
            ServiceKind::ALL
                .iter()
                .map(|k| k.name())
                .collect::<Vec<_>>()
                .join(" / ")
        )
    })
}

/// 把 `all` 或逗号列表展开成服务集合（仅管理类命令使用）。
///
/// `run` / `start` 故意只接受单个服务：进程隔离是刻意的部署选择，
/// 一进程多服务会让崩溃面与日志混在一起，需要时用 systemd / docker 多实例。
fn kinds_of(spec: &str) -> Result<Vec<ServiceKind>> {
    if spec.eq_ignore_ascii_case("all") {
        return Ok(ServiceKind::ALL.to_vec());
    }
    let mut out: Vec<ServiceKind> = Vec::new();
    for s in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let k = kind_of(s)?;
        if !out.contains(&k) {
            out.push(k);
        }
    }
    if out.is_empty() {
        bail!("服务名不能为空");
    }
    Ok(out)
}

/// 前台运行。
///
/// # Errors
/// 启动失败。
pub async fn run(service: &str, config_dir: &str) -> Result<()> {
    let kind = kind_of(service)?;
    Bootstrap::new(kind).with_config_dir(config_dir).run().await
}

/// 后台启动。
///
/// # Errors
/// 已在运行、fork 失败、重定向失败。
pub fn start(service: &str, config_dir: &str, workers: usize, paths: &Paths) -> Result<()> {
    let kind = kind_of(service)?;
    let svc = kind.name();
    paths.ensure().context("创建运行目录失败")?;

    if let Some(pid) = daemon::read_pid(&paths.pid(svc)) {
        bail!("{svc} 已在运行（pid={pid}），如需重启请用 ddj restart");
    }
    // 上次异常退出可能留下死 socket。
    let _ = std::fs::remove_file(paths.sock(svc));

    let Some(child) = daemon::fork().context("fork 失败")? else {
        // ---- 子进程 ----
        daemon::redirect_stdio(&paths.log(svc)).context("重定向标准流失败")?;
        let rt = build_rt(workers)?;
        let ret = rt.block_on(daemon_main(kind, config_dir, paths));
        if let Err(ref e) = ret {
            // 子进程的 stderr 已重定向到日志文件，这里必须显式落盘：
            // 否则 `ddj start` 只会静默退出，运维看不到任何原因。
            let msg = format!("{e:#}");
            tracing::error!(error = %msg, "守护进程启动失败");
            eprintln!("启动失败: {msg}");
            // 启动早期失败（数据库连不上等）时 pid 文件已经写了，必须清掉，
            // 否则 `ddj status` 会一直显示「进程在/控制通道断开」。
            daemon::cleanup(paths, kind.name());
        }
        // 守护进程退出必须走 _exit，避免刷新已被重定向的缓冲。
        let code = if ret.is_ok() { 0 } else { 1 };
        std::process::exit(code);
    };

    // ---- 父进程 ----
    println!("{svc} 已在后台启动 pid={child}");
    println!("  日志 : {}", paths.log(svc).display());
    println!("  控制 : ddj attach {svc}   |   ddj stop {svc}");
    Ok(())
}

/// 构建 tokio runtime。
fn build_rt(workers: usize) -> Result<tokio::runtime::Runtime> {
    let mut b = tokio::runtime::Builder::new_multi_thread();
    b.enable_all();
    if workers > 0 {
        b.worker_threads(workers);
    }
    b.build().context("创建 runtime 失败")
}

/// 守护进程主体。
async fn daemon_main(kind: ServiceKind, config_dir: &str, paths: &Paths) -> Result<()> {
    let svc = kind.name();
    let console = Console::new(Some(&paths.log(svc))).context("打开日志文件失败")?;
    // 必须在 Bootstrap::start() 之前安装：start() 内部会初始化日志订阅者。
    ddj_telemetry::set_sink(Arc::new(console.clone()));

    let pid = std::process::id();
    daemon::write_pid(&paths.pid(svc), i32::try_from(pid).unwrap_or(i32::MAX))
        .context("写 PID 文件失败")?;

    let mut running = Bootstrap::new(kind)
        .with_config_dir(config_dir)
        .start()
        .await?;

    let source: StatsSource = running.stats_source();
    let stats = Arc::new(RwLock::new(source.snapshot()));
    let state = Arc::new(ControlState {
        console,
        stats: Arc::clone(&stats),
        stop: running.stop_handle(),
        pid,
    });

    let sock = paths.sock(svc);
    let sock_for_task = sock.clone();
    tokio::spawn(async move {
        if let Err(e) = control::serve(&sock_for_task, state).await {
            tracing::error!(error = %e, "控制通道启动失败");
        }
    });

    // 每秒刷新一次指标快照，状态查询读它即可，不碰业务锁。
    let s = Arc::clone(&stats);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            *s.write().unwrap_or_else(|e| e.into_inner()) = source.snapshot();
        }
    });

    running.wait_stop().await;
    running.shutdown().await;
    daemon::cleanup(paths, svc);
    Ok(())
}

/// 停止。
///
/// # Errors
/// 未运行，或控制通道不可达。
pub async fn stop(service: &str, paths: &Paths) -> Result<()> {
    let kinds = kinds_of(service)?;
    if kinds.len() > 1 {
        let mut failed = Vec::new();
        for k in &kinds {
            // `stop all` 里没跑的服务直接跳过，不该算失败。
            if daemon::read_pid(&paths.pid(k.name())).is_none() {
                continue;
            }
            if let Err(e) = stop_one(k.name(), paths).await {
                failed.push(format!("{}: {e:#}", k.name()));
            }
        }
        if failed.is_empty() {
            return Ok(());
        }
        bail!("部分服务停止失败：\n  {}", failed.join("\n  "));
    }
    stop_one(kinds[0].name(), paths).await
}

/// 停止单个服务。
async fn stop_one(svc: &str, paths: &Paths) -> Result<()> {
    let sock = paths.sock(svc);

    if daemon::read_pid(&paths.pid(svc)).is_none() {
        bail!("{svc} 未在运行");
    }
    control::stop(&sock)
        .await
        .with_context(|| format!("控制通道不可达（{}），可用 kill 手动结束", sock.display()))?;

    // 等它真的退干净，避免「stop 之后立刻 start」撞上端口占用。
    for _ in 0..100 {
        if daemon::read_pid(&paths.pid(svc)).is_none() {
            println!("{svc} 已停止");
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    bail!(
        "{svc} 10 秒内未退出，请检查日志 {}",
        paths.log(svc).display()
    )
}

/// 状态（单个）。
///
/// # Errors
/// 服务名非法。
pub async fn status_one(service: &str, paths: &Paths) -> Result<()> {
    if service.eq_ignore_ascii_case("all") {
        return list(paths).await;
    }
    let kind = kind_of(service)?;
    let svc = kind.name();

    match control::status(&paths.sock(svc)).await {
        Ok(s) => {
            print_row(&s.stats, Some(s.pid), "运行中");
            // 单服务查询时额外打印分叉明细：真实客户端连上来后，
            // 这里非 0 就是「碰到了畸形数据」的第一手信号。
            print_divergences(&s.stats.divergences);
        }
        Err(_) => match daemon::read_pid(&paths.pid(svc)) {
            Some(pid) => print_row(
                &empty_stats(svc),
                Some(pid.cast_unsigned()),
                "进程在/控制通道断开",
            ),
            None => print_row(&empty_stats(svc), None, "未运行"),
        },
    }
    Ok(())
}

/// 打印三处「偏离原版」护栏的触发次数。
///
/// 全 0 时只打一行提示；非 0 时逐条列出，并指出去哪里看日志。
fn print_divergences(list: &[ddj_game_core::divergence::DivergenceCount]) {
    if list.is_empty() {
        return;
    }
    println!();
    if list.iter().all(|d| d.count == 0) {
        println!("偏离原版护栏：全部未触发（正常）");
        return;
    }
    println!("偏离原版护栏（AGENTS.md §3）：");
    for d in list.iter().filter(|d| d.count > 0) {
        println!(
            "  {} ×{}  原版：{}",
            d.divergence.code(),
            d.count,
            d.divergence.original()
        );
    }
    println!("  定位：日志搜 target=ddj::divergence，详见 docs/AUDIT.md「待裁决偏离」");
}

/// 列出全部。
///
/// # Errors
/// 无。
pub async fn list(paths: &Paths) -> Result<()> {
    println!(
        "{:<10} {:<8} {:>8} {:>9} {:>9} {:>9}  状态",
        "服务", "PID", "运行时长", "在线", "处理器", "配置表"
    );
    println!("{}", "-".repeat(78));
    for k in ServiceKind::ALL {
        let svc = k.name();
        match control::status(&paths.sock(svc)).await {
            Ok(s) => print_row(&s.stats, Some(s.pid), "运行中"),
            Err(_) => match daemon::read_pid(&paths.pid(svc)) {
                Some(pid) => {
                    print_row(&empty_stats(svc), Some(pid.cast_unsigned()), "控制通道断开")
                }
                None => print_row(&empty_stats(svc), None, "未运行"),
            },
        }
    }
    Ok(())
}

fn empty_stats(svc: &str) -> Stats {
    Stats {
        service: svc.to_owned(),
        uptime_ms: 0,
        online: 0,
        handlers: 0,
        templates: 0,
        divergences: Vec::new(),
    }
}

fn fmt_uptime(ms: u64) -> String {
    if ms == 0 {
        return "-".into();
    }
    let s = ms / 1000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m{}s", s / 60, s % 60)
    } else {
        format!("{}h{}m", s / 3600, (s % 3600) / 60)
    }
}

fn print_row(st: &Stats, pid: Option<u32>, state: &str) {
    let pid_s = pid.map_or_else(|| "-".to_owned(), |p| p.to_string());
    println!(
        "{:<10} {:<8} {:>8} {:>9} {:>9} {:>9}  {}",
        st.service,
        pid_s,
        fmt_uptime(st.uptime_ms),
        st.online,
        st.handlers,
        st.templates,
        state
    );
}

/// 连上实时输出。
///
/// # Errors
/// 未运行。
pub async fn attach(service: &str, tail: usize, paths: &Paths) -> Result<()> {
    let kind = kind_of(service)?;
    let sock = paths.sock(kind.name());
    control::attach(&sock, tail)
        .await
        .with_context(|| format!("连接 {} 失败，服务可能未以 Start 方式运行", sock.display()))
}

/// 打印日志文件。
///
/// # Errors
/// 文件不存在。
pub async fn logs(service: &str, lines: usize, follow: bool, paths: &Paths) -> Result<()> {
    let kind = kind_of(service)?;
    let path = paths.log(kind.name());
    if !path.exists() {
        bail!("日志文件不存在：{}", path.display());
    }
    let mut f = std::fs::File::open(&path)?;
    let len = f.metadata()?.len();
    let start = last_lines_offset(&mut f, len, lines)?;
    f.seek(SeekFrom::Start(start))?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)?;
    print!("{buf}");
    std::io::stdout().flush().ok();

    if !follow {
        return Ok(());
    }
    let mut pos = len;
    loop {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let cur = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(pos);
        if cur > pos {
            f.seek(SeekFrom::Start(pos))?;
            let mut chunk = String::new();
            f.read_to_string(&mut chunk)?;
            print!("{chunk}");
            std::io::stdout().flush().ok();
            pos = cur;
        }
    }
}

/// 估算最后 `n` 行的字节偏移（从尾部往回扫 `\n`）。
fn last_lines_offset(f: &mut std::fs::File, len: u64, n: usize) -> std::io::Result<u64> {
    const CHUNK: u64 = 8192;
    let mut want = n + 1;
    let mut end = len;
    let mut buf = vec![0u8; CHUNK as usize];
    while end > 0 && want > 0 {
        let read = CHUNK.min(end);
        let start = end - read;
        f.seek(SeekFrom::Start(start))?;
        f.read_exact(&mut buf[..read as usize])?;
        for b in buf[..read as usize].iter().rev() {
            if *b == b'\n' {
                want -= 1;
                if want == 0 {
                    return Ok(start + read);
                }
            }
        }
        end = start;
    }
    Ok(0)
}

/// 列出服务名。
pub fn services() {
    for k in ServiceKind::ALL {
        println!("{:<10} 端口逻辑见 config/{}.toml", k.name(), k.name());
    }
}

/// 判断是否已有实例在跑（供 restart 复用）。
pub fn running_pid(service: &str, paths: &Paths) -> Option<i32> {
    daemon::read_pid(&paths.pid(service))
}
