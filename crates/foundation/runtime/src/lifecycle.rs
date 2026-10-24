//! 生命周期编排

use crate::context::ServiceContext;
use crate::module::{Module, Phase};
use crate::registry::ModuleEntry;
use ddj_core::Result;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 编排器。
pub struct Lifecycle {
    modules: Vec<Box<dyn Module>>,
    /// 单阶段超时。
    timeout: Duration,
}

impl Lifecycle {
    /// 从注册项构造。
    #[must_use]
    pub fn from_entries(entries: &[ModuleEntry]) -> Self {
        Self {
            modules: entries.iter().map(|e| (e.factory)()).collect(),
            timeout: Duration::from_secs(30),
        }
    }

    /// 设置阶段超时。
    #[must_use]
    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    /// 模块名列表。
    #[must_use]
    pub fn names(&self) -> Vec<&'static str> {
        self.modules.iter().map(|m| m.name()).collect()
    }

    /// 拓扑排序：依赖在前。
    ///
    /// # Errors
    /// 依赖名不存在、或成环。
    pub fn topological(&self) -> Result<Vec<usize>> {
        let idx: HashMap<&'static str, usize> = self
            .modules
            .iter()
            .enumerate()
            .map(|(i, m)| (m.name(), i))
            .collect();

        let n = self.modules.len();
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut indeg = vec![0usize; n];

        for (i, m) in self.modules.iter().enumerate() {
            for dep in m.depends_on() {
                let Some(&j) = idx.get(dep) else {
                    return Err(ddj_core::AppError::BadRequest(format!(
                        "模块 {} 依赖不存在的模块 {dep}",
                        m.name()
                    )));
                };
                adj[j].push(i);
                indeg[i] += 1;
            }
        }

        let mut queue: std::collections::VecDeque<usize> =
            (0..n).filter(|i| indeg[*i] == 0).collect();
        let mut order = Vec::with_capacity(n);
        while let Some(u) = queue.pop_front() {
            order.push(u);
            for &v in &adj[u] {
                indeg[v] -= 1;
                if indeg[v] == 0 {
                    queue.push_back(v);
                }
            }
        }
        if order.len() != n {
            return Err(ddj_core::AppError::BadRequest("模块依赖成环".into()));
        }
        Ok(order)
    }

    /// 按阶段启动全部模块。
    ///
    /// 任一模块失败则**停止后续启动**并反序 stop 已启动的模块。
    pub async fn start_all(&mut self, _ctx: &ServiceContext) -> Result<()> {
        let order = self.topological()?;

        for phase in [Phase::PreInit, Phase::Init, Phase::Start] {
            for &i in &order {
                let name = self.modules[i].name();
                let t0 = Instant::now();
                let r = match phase {
                    Phase::PreInit => self.modules[i].pre_init().await,
                    Phase::Init => self.modules[i].init().await,
                    Phase::Start => self.modules[i].start().await,
                    Phase::Ready => Ok(()),
                };
                match r {
                    Ok(()) => tracing::info!(
                        module = name,
                        phase = ?phase,
                        elapsed_ms = t0.elapsed().as_millis(),
                        "模块启动"
                    ),
                    Err(e) => {
                        tracing::error!(module = name, phase = ?phase, error = %e, "模块启动失败");
                        self.stop_all(&order).await;
                        return Err(e);
                    }
                }
            }
            tracing::info!(phase = ?phase, "阶段完成");
        }
        Ok(())
    }

    /// 反序停机。
    pub async fn stop_all(&mut self, order: &[usize]) {
        for &i in order.iter().rev() {
            let name = self.modules[i].name();
            if let Err(e) = self.modules[i].stop().await {
                tracing::warn!(module = name, error = %e, "模块停机出错");
            }
        }
    }
}
