//! 路由装配
//!
//! 保留原 PHP 的 URL（`/i.php` `/l.php` `/p/p.php` `/gm/...`）以兼容现有客户端，
//! 同时提供 RESTful 新路径，两者指向同一 handler。

use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;

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

async fn gm_pay() -> Json<Value> {
    Json(json!({ "status": 200, "note": "原转发到 http://127.0.0.1:1544/gs/pay" }))
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
