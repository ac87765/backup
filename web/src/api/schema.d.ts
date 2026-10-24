/**
 * 自动生成，请勿手改。
 * 来源：crates/server/src/openapi.rs（`cargo run -p server -- openapi`）。
 * 重新生成：cd web && npm run generate:api
 */
export interface paths {
    "/healthz": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** 存活检查。 */
        get: operations["healthz"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/readyz": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** 就绪检查。 */
        get: operations["readyz"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
}
export type webhooks = Record<string, never>;
export interface components {
    schemas: {
        /** @description API 错误响应体（对外契约）。 */
        ErrorBody: {
            /** @description 稳定的机器可读错误码。 */
            code: string;
            /** @description 面向用户的通用文案。 */
            message: string;
            /** @description 服务端为本请求生成的 ID，便于运维检索日志。 */
            request_id: string;
        };
        /** @description 存活检查响应。 */
        HealthResponse: {
            status: string;
        };
        /** @description 就绪检查各子项状态。 */
        ReadyChecks: {
            database: string;
            migrations: string;
            storage: string;
        };
        /** @description 就绪检查响应。 */
        ReadyResponse: {
            status: string;
            checks: components["schemas"]["ReadyChecks"];
        };
    };
    responses: never;
    parameters: never;
    requestBodies: never;
    headers: never;
    pathItems: never;
}
export type $defs = Record<string, never>;
export interface operations {
    healthz: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description 进程存活 */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["HealthResponse"];
                };
            };
        };
    };
    readyz: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description 服务就绪 */
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ReadyResponse"];
                };
            };
            /** @description 依赖不可用或迁移版本不匹配 */
            503: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ReadyResponse"];
                };
            };
        };
    };
}
