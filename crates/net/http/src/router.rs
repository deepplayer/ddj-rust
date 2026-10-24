//! 路由装配
//!
//! 保留原 PHP 的 URL（`/i.php` `/l.php` `/p/p.php` `/gm/...`）以兼容现有客户端，
//! 同时提供 RESTful 新路径，两者指向同一 handler。

use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock};

use crate::gm::{self, GmAction};
use crate::middleware::RateLimiter;

/// 路由共享状态。
#[derive(Clone)]
pub struct HttpState {
    /// 版本号（用于 `/healthz`）。
    pub version: &'static str,
    /// 区服 ID。
    pub server_id: i32,
    /// 限流器。
    pub limiter: Arc<RateLimiter>,
}

/// 组装全部路由。
pub fn build_router(state: HttpState) -> Router {
    let ops = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics));

    // 兼容原 PHP 的路径（客户端硬编码，不能改名）
    let legacy = Router::new()
        .route("/i.php", post(sdk_config).get(sdk_config))
        .route("/l.php", post(sdk_login))
        .route("/p/p.php", post(sdk_pay))
        .route("/login/index.php", post(sdk_login))
        .route("/gm/gmPay.php", post(gm_pay))
        .route("/gm/cdk/cdkPay.php", post(gm_cdk_pay))
        .route("/gm/cdk/genCdk.php", post(gm_cdk_gen));

    // 新的 RESTful 路径
    let v1 = Router::new()
        .route("/sdk/v1/config", get(sdk_config))
        .route("/sdk/v1/login", post(sdk_login))
        .route("/sdk/v1/pay", post(sdk_pay))
        .route("/gm/v1/charge", post(gm_pay))
        .route("/gm/v1/cdk/redeem", post(gm_cdk_pay))
        .route("/gm/v1/cdk/generate", post(gm_cdk_gen))
        .route("/gm/v1/players", get(gm_players));

    Router::new()
        .merge(ops)
        .merge(legacy)
        .nest("/api", v1)
        .layer(axum::middleware::from_fn(crate::middleware::trace_layer))
        .with_state(state)
}

async fn healthz() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn readyz(axum::extract::State(s): axum::extract::State<HttpState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "version": s.version,
        "server_id": s.server_id,
    }))
}

async fn metrics() -> &'static str {
    "# 由 ddj-telemetry 采集后在此导出 Prometheus 文本格式\n"
}

async fn sdk_config() -> Json<Value> {
    Json(json!({
        "status": 200,
        "data": {
            "enkey": "",
            "note": "AES-128-ECB，密钥由 ddj-config 注入，此处不含明文",
        }
    }))
}

async fn sdk_login(Json(body): Json<Value>) -> Json<Value> {
    Json(json!({ "status": 200, "echo": body, "note": "LoginService 实现待接入" }))
}

async fn sdk_pay() -> Json<Value> {
    Json(json!({ "status": 200 }))
}

/// GM 后台的数据依赖：查玩家是否存在、查 CDK 权限、把原始 body 转发到 jetty。
///
/// 用 trait 而不是直接把连接池塞进 `HttpState`，是因为这两个查询落在**不同的库**：
/// `player` 在游戏库、`user_cdk` 在 `platform` 库。
#[async_trait::async_trait]
pub trait GmBackend: Send + Sync {
    /// `SELECT 1 FROM player WHERE id = $1`
    async fn player_exists(&self, userid: &str) -> Result<bool, String>;
    /// `SELECT 1 FROM user_cdk WHERE userid = $1`
    async fn has_cdk(&self, userid: &str) -> Result<bool, String>;
    /// 原版 `sendPostRequest("http://127.0.0.1:1544" + path, $code)`，返回值被丢弃。
    async fn push(&self, path: &str, body: &str) -> Result<(), String>;
}

static GM_BACKEND: OnceLock<Arc<dyn GmBackend>> = OnceLock::new();

/// 由 `game-core` 在装配阶段注入实现。重复调用被忽略（进程内只允许一个）。
pub fn install_gm_backend(b: Arc<dyn GmBackend>) {
    let _ = GM_BACKEND.set(b);
}

/// `gmPay.php` 的表单字段。
#[derive(Debug, Deserialize)]
pub struct GmForm {
    #[serde(default)]
    userid: String,
    #[serde(default)]
    request_type: String,
    #[serde(default)]
    code: String,
}

/// `POST /gm/gmPay.php` 与 `POST /api/gm/v1/charge`。
///
/// 判定与响应文案完全走 [`gm::decide`] / [`gm::body_of`]，这里只做 IO。
async fn gm_pay(Form(form): Form<GmForm>) -> Response {
    let Some(backend) = GM_BACKEND.get() else {
        return (
            axum::http::StatusCode::NOT_IMPLEMENTED,
            Json(json!({
                "status": "error",
                "message": "GM backend 未装配：需先 install_gm_backend()",
            })),
        )
            .into_response();
    };
    let action = gm::parse_action(&form.request_type);
    let player_exists = match backend.player_exists(&form.userid).await {
        Ok(v) => v,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "status": "error", "message": e })),
            )
                .into_response();
        }
    };
    // 玩家不存在时原版不会去查 CDK，这里同样短路，避免无谓查询。
    let has_cdk = if player_exists {
        match backend.has_cdk(&form.userid).await {
            Ok(v) => v,
            Err(e) => {
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "status": "error", "message": e })),
                )
                    .into_response();
            }
        }
    } else {
        false
    };

    let decision = gm::decide(action, player_exists, has_cdk);
    if let gm::GmDecision::Push(act) = decision {
        let path = match act {
            GmAction::Daijin => "/gs/pushPlayerReplaceMoneyNum2",
            GmAction::Mail => "/gs/sendMail",
            GmAction::Tingfu => "",
        };
        // 原版丢弃下游结果，这里只记不报。
        if let Err(e) = backend.push(path, &form.code).await {
            tracing::warn!(path, error = %e, "GM 下游转发失败，按原版仍回成功");
        }
    }

    let code = gm::status_of(decision);
    let status = axum::http::StatusCode::from_u16(code)
        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    match gm::body_of(decision) {
        Some(v) => (status, Json(v)).into_response(),
        // tingfu：原版 200 且响应体为空。
        None => status.into_response(),
    }
}

async fn gm_cdk_pay() -> Json<Value> {
    Json(json!({ "status": 200 }))
}

async fn gm_cdk_gen() -> Json<Value> {
    Json(json!({ "status": 200 }))
}

async fn gm_players() -> Json<Value> {
    Json(json!({ "status": 200, "rows": [] }))
}
