//! OpenAPI 规范生成。
//!
//! `server openapi` 把该规范输出到 stdout；`web/scripts/generate-api.mjs` 用它生成
//! TypeScript 类型（`web/src/api/schema.d.ts`），CI 会重新生成并检查无漂移。

use utoipa::OpenApi;

use crate::api::error::ErrorBody;
use crate::api::health::{HealthResponse, ReadyChecks, ReadyResponse};

/// 服务端 API 文档。
#[derive(OpenApi)]
#[openapi(
    info(
        title = "端对端加密备份服务 API",
        version = "0.1.0",
        description = "服务端只保存密文与元数据；所有加密操作在浏览器 Worker 中完成。"
    ),
    paths(crate::api::health::healthz, crate::api::health::readyz),
    components(schemas(ErrorBody, HealthResponse, ReadyChecks, ReadyResponse))
)]
pub struct ApiDoc;

/// 生成 OpenAPI 规范。
pub fn spec() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

/// 生成便于人读、稳定排序的 JSON。
pub fn spec_json_pretty() -> Result<String, serde_json::Error> {
    spec().to_pretty_json()
}
