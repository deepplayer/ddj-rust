//! 守护化
//!
//! 目标：`ddj start game` 之后，进程**完全脱离启动它的 shell**
//! （换会话、SSH 掉线、终端关闭都不影响），同时保留一个控制入口。
//!
//! ## 为什么单次 fork 就够
//! 常见的「双 fork」是为了防止进程再次获得控制终端。这里 `setsid()` 之后
//! 进程没有控制终端，且我们从不 `open("/dev/tty")`，所以单次 fork 已满足。
//! 单次 fork 的好处是**父进程能立刻拿到最终 PID 并打印**，双 fork 拿不到。
//!
//! ## 时序约束
//! `fork` 必须在创建 tokio runtime **之前**完成。多线程进程 fork 后，
//! 子进程只保留调用线程，其他线程持有的锁可能永久不可用。

// 全仓库仅此一处放开 unsafe：守护化必须用 fork / setsid / dup2 / signal，
// 标准库没有安全等价物。所有调用点都写了 SAFETY 注释。
#![allow(unsafe_code)]

use std::ffi::CString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// 路径集合。
pub struct Paths {
    /// 运行目录。
    pub dir: PathBuf,
}

impl Paths {
    /// 解析运行目录。
    ///
    /// 优先级：显式参数 > `DDJ_RUN_DIR` > `$XDG_RUNTIME_DIR/ddj` > `/tmp/ddj-<uid>`。
    #[must_use]
    pub fn resolve(explicit: Option<&str>) -> Self {
        if let Some(d) = explicit {
            return Self {
                dir: PathBuf::from(d),
            };
        }
        if let Ok(d) = std::env::var("XDG_RUNTIME_DIR") {
            if !d.is_empty() {
                return Self {
                    dir: PathBuf::from(d).join("ddj"),
                };
            }
        }
        Self {
            dir: PathBuf::from(format!("/tmp/ddj-{}", uid())),
        }
    }

    /// 确保目录存在。
    ///
    /// # Errors
    /// 目录无法创建。
    pub fn ensure(&self) -> io::Result<()> {
        fs::create_dir_all(&self.dir)
    }

    /// PID 文件。
    #[must_use]
    pub fn pid(&self, svc: &str) -> PathBuf {
        self.dir.join(format!("{svc}.pid"))
    }

    /// 控制 socket。
    #[must_use]
    pub fn sock(&self, svc: &str) -> PathBuf {
        self.dir.join(format!("{svc}.sock"))
    }

    /// 日志文件。
    #[must_use]
    pub fn log(&self, svc: &str) -> PathBuf {
        self.dir.join(format!("{svc}.log"))
    }
}

/// 当前用户 ID。
#[must_use]
pub fn uid() -> u32 {
    // SAFETY: getuid 无副作用，永远成功。
    unsafe { libc::getuid() }
}

/// 进程是否存活。
#[must_use]
pub fn alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    // SAFETY: kill(pid, 0) 只做权限与存在性检查，不发送信号。
    unsafe { libc::kill(pid, 0) == 0 }
}

/// 读 PID 文件，返回仍然存活的 PID。
#[must_use]
pub fn read_pid(path: &Path) -> Option<i32> {
    let s = fs::read_to_string(path).ok()?;
    let pid: i32 = s.lines().next()?.trim().parse().ok()?;
    alive(pid).then_some(pid)
}

/// 写 PID 文件。
///
/// # Errors
/// 写入失败。
pub fn write_pid(path: &Path, pid: i32) -> io::Result<()> {
    fs::write(path, format!("{pid}\n"))
}

/// 清理运行期文件（pid / socket）。
pub fn cleanup(paths: &Paths, svc: &str) {
    let _ = fs::remove_file(paths.pid(svc));
    let _ = fs::remove_file(paths.sock(svc));
}

/// fork 一次；父进程返回子进程 PID，子进程返回 `None` 并继续执行。
///
/// # Errors
/// fork 失败。
pub fn fork() -> io::Result<Option<u32>> {
    // SAFETY: 调用点保证在创建任何线程之前。
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(io::Error::last_os_error());
    }
    if pid > 0 {
        return Ok(Some(pid as u32));
    }
    // 子进程：新建会话，脱离控制终端。
    // SAFETY: setsid 在当前进程不是进程组组长时成功（fork 后必然成立）。
    if unsafe { libc::setsid() } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(None)
}

/// 把 stdin/stdout/stderr 重定向：stdin→/dev/null，stdout/stderr→日志文件。
///
/// # Errors
/// 打开或 dup 失败。
pub fn redirect_stdio(log: &Path) -> io::Result<()> {
    let log_c = cstr(log)?;
    // SAFETY: 参数均为合法指针/常量；失败时返回负值，已逐项检查。
    unsafe {
        let devnull = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
        if devnull < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::dup2(devnull, libc::STDIN_FILENO) < 0 {
            return Err(io::Error::last_os_error());
        }
        let fd = libc::open(
            log_c.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND,
            0o644,
        );
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        libc::dup2(fd, libc::STDOUT_FILENO);
        libc::dup2(fd, libc::STDERR_FILENO);
        if fd > 2 {
            libc::close(fd);
        }
        if devnull > 2 {
            libc::close(devnull);
        }
        libc::umask(0o027);
        // SSH 断开时内核会给会话前导进程发 SIGHUP；忽略它，让进程活下来。
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    Ok(())
}

fn cstr(p: &Path) -> io::Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(p.as_os_str().as_bytes()).map_err(|_| io::Error::other("路径含 NUL"))
}
