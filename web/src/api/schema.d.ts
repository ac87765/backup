/**
 * 自动生成，请勿手改。
 * 来源：crates/server/src/openapi.rs（`cargo run -p server -- openapi`）。
 * 重新生成：cd web && npm run generate:api
 */
export interface paths {
    "/api/v1/admin/audit": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["list_audit"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/overview": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["overview"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/settings": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["get_settings"];
        put: operations["update_settings"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/users": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["list_users"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/users/{user_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["get_user"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/users/{user_id}/approve": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["approve_user"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/users/{user_id}/quota": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put: operations["update_quota"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/users/{user_id}/reactivate": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["reactivate_user"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/users/{user_id}/reject": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["reject_user"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/users/{user_id}/revoke-sessions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["revoke_user_sessions"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/admin/users/{user_id}/suspend": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["suspend_user"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/auth/change-password": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["change_password"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/auth/login": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["login"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/auth/logout": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["logout"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/auth/me": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["me"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/auth/prelogin": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["prelogin"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/auth/recover": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["recover"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/auth/recovery-prelogin": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * 恢复重置前置查询。
         * @description `recovery_wrapped_mk` 是密文，但忘记密码的用户在认证前就需要它；
         *     对不存在邮箱返回结构相同的伪造帧，避免泄露邮箱是否注册。
         */
        post: operations["recovery_prelogin"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/auth/register": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["register"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/auth/resend-verification": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["resend_verification"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/auth/rotate-recovery-key": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["rotate_recovery_key"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/auth/sessions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["list_sessions"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/auth/sessions/{session_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        delete: operations["revoke_session"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/auth/verify-email": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["verify_email"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/folders": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * `POST /api/v1/folders`。
         * @description 同一客户端 ID + 同一内容重复提交返回同一结果；同一 ID 不同内容返回
         *     `conflict`。同名冲突由部分唯一索引兜底。
         */
        post: operations["create_folder"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/me/usage": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** `GET /api/v1/me/usage`。 */
        get: operations["get_usage"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/nodes/{node_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /** `DELETE /api/v1/nodes/{node_id}`：把节点及其子树移入回收站。 */
        delete: operations["delete_node"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/nodes/{node_id}/children": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** `GET /api/v1/nodes/{node_id}/children`。 */
        get: operations["list_children"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/nodes/{node_id}/move": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        /**
         * `PATCH /api/v1/nodes/{node_id}/move`。
         * @description 事务内先锁 `users` 行，使同一用户的移动串行化；再用递归 CTE 检查目标
         *     不是自身或后代。这样并发 A→B 与 B→A 最多只有一个成功，不会形成环。
         */
        patch: operations["move_node"];
        trace?: never;
    };
    "/api/v1/nodes/{node_id}/rename": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        /** `PATCH /api/v1/nodes/{node_id}/rename`。 */
        patch: operations["rename_node"];
        trace?: never;
    };
    "/api/v1/nodes/{node_id}/versions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** `GET /api/v1/nodes/{node_id}/versions`。 */
        get: operations["list_versions"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/nodes/{node_id}/versions/restore": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * `POST /api/v1/nodes/{node_id}/versions/restore`。
         * @description 恢复会新建一个指向原 content 的版本并把它设为当前版本；旧版本仍保留。
         */
        post: operations["restore_version"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/nodes/{node_id}/versions/{version_id}/download": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** `GET /api/v1/nodes/{node_id}/versions/{version_id}/download`。 */
        get: operations["download_version"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/trash": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** `GET /api/v1/trash`：只列每个 batch 的顶层项。 */
        get: operations["list_trash"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/trash/{node_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        /** `DELETE /api/v1/trash/{node_id}`：永久删除一个回收站项目。 */
        delete: operations["permanently_delete_node"];
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/trash/{node_id}/restore": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** `POST /api/v1/trash/{node_id}/restore`。 */
        post: operations["restore_node"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/upload-sessions": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * `POST /api/v1/upload-sessions`。
         * @description 创建时在同一事务内锁定用户行、检查邮箱验证状态并预留配额。
         *     `content_id` 是幂等键：同一请求重复提交返回同一会话且不重复预留；
         *     同一 `content_id` 不同内容返回 `conflict`。
         */
        post: operations["create_upload_session"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/upload-sessions/{session_id}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /** `GET /api/v1/upload-sessions/{session_id}`。 */
        get: operations["get_upload_session"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/upload-sessions/{session_id}/abort": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** `POST /api/v1/upload-sessions/{session_id}/abort`。 */
        post: operations["abort_upload"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/upload-sessions/{session_id}/chunks": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        /**
         * `GET /api/v1/upload-sessions/{session_id}/chunks`。
         * @description 只返回已确认的块；客户端据此只上传缺失块（断点续传）。
         */
        get: operations["list_confirmed_chunks"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/upload-sessions/{session_id}/chunks/confirm": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * `POST /api/v1/upload-sessions/{session_id}/chunks/confirm`。
         * @description 一次确认最多 [`MAX_BATCH_CHUNKS`] 个块；逐块独立结果，部分失败不影响其他块。
         */
        post: operations["confirm_chunks"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/upload-sessions/{session_id}/chunks/presign": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * `POST /api/v1/upload-sessions/{session_id}/chunks/presign`。
         * @description 一次为最多 [`MAX_BATCH_CHUNKS`] 个块生成 URL；逐块独立结果。
         */
        post: operations["presign_chunks"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/upload-sessions/{session_id}/chunks/{idx}/confirm": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * `POST /api/v1/upload-sessions/{session_id}/chunks/{idx}/confirm`。
         * @description 服务端向对象存储 `stat` 实际大小，必须等于该块期望大小；相同块、相同哈希
         *     的重复确认幂等成功。
         */
        post: operations["confirm_chunk"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/upload-sessions/{session_id}/chunks/{idx}/presign": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /**
         * `POST /api/v1/upload-sessions/{session_id}/chunks/{idx}/presign`。
         * @description 每次调用都生成新的服务端对象键；旧键写入 `superseded_object_keys`，
         *     在提交/中止/过期时统一进入删除队列。已确认状态会被重置（新键尚未上传）。
         */
        post: operations["presign_chunk"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/upload-sessions/{session_id}/commit": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        /** `POST /api/v1/upload-sessions/{session_id}/commit`。 */
        post: operations["commit_upload"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
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
        /** @description 中止上传响应。 */
        AbortUploadResponse: {
            /** Format: uuid */
            session_id: string;
            status: string;
        };
        AdminAuditEntry: {
            /** Format: int64 */
            id: number;
            /** Format: uuid */
            admin_id: string;
            action: string;
            /** Format: uuid */
            target_user_id?: string | null;
            details: unknown;
            ip: string;
            created_at: string;
        };
        AdminAuditQuery: {
            cursor?: string | null;
            /** Format: uuid */
            target_user_id?: string | null;
            /** Format: int64 */
            limit?: number | null;
        };
        AdminAuditResponse: {
            items: components["schemas"]["AdminAuditEntry"][];
            /** Format: int64 */
            next_cursor?: number | null;
        };
        AdminHealth: {
            status: string;
            database: string;
            storage: string;
        };
        AdminOkResponse: {
            ok: boolean;
        };
        AdminOverview: {
            /** Format: int64 */
            pending_users: number;
            status_counts: components["schemas"]["AdminStatusCounts"];
            /** Format: int64 */
            total_used_bytes: number;
            /** Format: int64 */
            total_reserved_bytes: number;
            /** Format: int64 */
            total_quota_bytes: number;
            /** Format: int64 */
            registrations_last_7_days: number;
            queues: components["schemas"]["AdminQueueBacklog"];
            health: components["schemas"]["AdminHealth"];
            version: string;
        };
        AdminQueueBacklog: {
            /** Format: int64 */
            email_outbox: number;
            /** Format: int64 */
            object_deletions: number;
        };
        AdminSessionSummary: {
            /** Format: uuid */
            id: string;
            created_at: string;
            last_seen_at: string;
            ip: string;
            user_agent?: string | null;
            revoked: boolean;
        };
        AdminSettingsResponse: {
            registration_mode: string;
            /** Format: int64 */
            default_quota_bytes: number;
        };
        AdminStatusCounts: {
            /** Format: int64 */
            pending: number;
            /** Format: int64 */
            active: number;
            /** Format: int64 */
            suspended: number;
            /** Format: int64 */
            rejected: number;
        };
        AdminUserDetailResponse: {
            user: components["schemas"]["AdminUserSummary"];
            sessions: components["schemas"]["AdminSessionSummary"][];
            audit: components["schemas"]["AdminAuditEntry"][];
        };
        AdminUserListResponse: {
            items: components["schemas"]["AdminUserSummary"][];
            next_cursor?: string | null;
        };
        AdminUserSummary: {
            /** Format: uuid */
            id: string;
            email: string;
            status: string;
            is_admin: boolean;
            created_at: string;
            email_verified_at?: string | null;
            approved_at?: string | null;
            last_active_at?: string | null;
            /** Format: int64 */
            used_bytes: number;
            /** Format: int64 */
            reserved_bytes: number;
            /** Format: int64 */
            quota_bytes: number;
            /**
             * Format: int64
             * @description 未删除的文件数（不含根文件夹）。
             */
            file_count: number;
            /**
             * Format: int64
             * @description 未删除的文件夹数（不含根文件夹）。
             */
            folder_count: number;
            /** Format: int64 */
            version_count: number;
            /** Format: int64 */
            active_session_count: number;
        };
        AdminUsersQuery: {
            status?: string | null;
            /** @description 邮箱子串（ILIKE，转义 `%`/`_`）。 */
            q?: string | null;
            cursor?: string | null;
            /** Format: int64 */
            limit?: number | null;
        };
        ApproveUserRequest: {
            /**
             * Format: int64
             * @description 缺省时使用 app_settings.default_quota_bytes（再回退到配置）。
             */
            quota_bytes?: number | null;
        };
        ChangePasswordRequest: {
            old_auth_key: string;
            new_auth_key: string;
            new_kdf_params: components["schemas"]["KdfParams"];
            new_kdf_salt: string;
            new_wrapped_mk: string;
        };
        /** @description 列目录响应。客户端解密名称后在本地排序。 */
        ChildrenResponse: {
            items: components["schemas"]["NodeSummary"][];
            /**
             * Format: uuid
             * @description 为 `null` 表示没有更多页。
             */
            next_cursor?: string | null;
        };
        /** @description 提交上传响应。重复提交返回完全相同的字段。 */
        CommitUploadResponse: {
            /** Format: uuid */
            session_id: string;
            status: string;
            /** Format: uuid */
            node_id: string;
            /** Format: uuid */
            version_id: string;
            /** Format: uuid */
            content_id: string;
            /** Format: int64 */
            ciphertext_size: number;
            /** Format: int32 */
            chunk_count: number;
            committed_at: string;
        };
        /** @description 批量确认中的一块。 */
        ConfirmChunkItem: {
            /** Format: int32 */
            idx: number;
            /** Format: int64 */
            declared_size: number;
            sha256: string;
        };
        /** @description 批量确认中的一项结果。 */
        ConfirmChunkItemResult: {
            /** Format: int32 */
            idx: number;
            ok: boolean;
            /** Format: int64 */
            declared_size?: number | null;
            sha256?: string | null;
            confirmed_at?: string | null;
            code?: string | null;
            message?: string | null;
        };
        /** @description 块确认请求。 */
        ConfirmChunkRequest: {
            /**
             * Format: int64
             * @description 客户端声明的密文字节数，必须等于服务端推出的期望大小。
             */
            declared_size: number;
            /** @description 客户端计算的密文 SHA-256（hex，32 字节）。 */
            sha256: string;
        };
        /** @description 块确认响应。 */
        ConfirmChunkResponse: {
            /** Format: int32 */
            idx: number;
            /** Format: int64 */
            declared_size: number;
            sha256: string;
            confirmed_at: string;
        };
        /** @description 批量确认请求。一次最多 [`MAX_BATCH_CHUNKS`] 个块。 */
        ConfirmChunksRequest: {
            chunks: components["schemas"]["ConfirmChunkItem"][];
        };
        /** @description 批量确认响应。逐块独立结果，部分失败不影响其他块。 */
        ConfirmChunksResponse: {
            /** Format: uuid */
            session_id: string;
            items: components["schemas"]["ConfirmChunkItemResult"][];
        };
        /** @description 已确认块列表中的一项。 */
        ConfirmedChunk: {
            /** Format: int32 */
            idx: number;
            /** Format: int64 */
            declared_size: number;
            sha256: string;
            confirmed_at: string;
        };
        /** @description 已确认块列表响应（供断点续传）。 */
        ConfirmedChunksResponse: {
            /** Format: uuid */
            session_id: string;
            /** Format: int32 */
            chunk_count: number;
            /** Format: int64 */
            confirmed_count: number;
            items: components["schemas"]["ConfirmedChunk"][];
        };
        /** @description 创建文件夹请求。`id` 是客户端生成的幂等键。 */
        CreateFolderRequest: {
            /** Format: uuid */
            id: string;
            /** Format: uuid */
            parent_id: string;
            /** @description 包裹的节点密钥（hex）。 */
            wrapped_key: string;
            /** @description 加密元数据（hex）。 */
            encrypted_meta: string;
            /** @description 名称令牌（hex）。 */
            name_token: string;
        };
        /** @description 创建上传会话请求。 */
        CreateUploadSessionRequest: {
            /**
             * Format: uuid
             * @description 客户端生成的内容 ID（UUIDv4），创建请求的幂等键。
             */
            content_id: string;
            /** @description `new_file` 或 `new_version`。 */
            kind: string;
            /**
             * Format: uuid
             * @description 新文件：即将创建的节点 ID；新版本：已有文件的节点 ID。
             */
            node_id: string;
            /**
             * Format: uuid
             * @description 仅新文件：目标父文件夹 ID。
             */
            parent_id?: string | null;
            /** @description 仅新文件：包裹的节点密钥（hex）。 */
            wrapped_key?: string | null;
            /** @description 仅新文件：加密节点元数据（hex）。 */
            encrypted_meta?: string | null;
            /** @description 仅新文件：名称令牌（hex）。 */
            name_token?: string | null;
            /**
             * Format: uuid
             * @description 客户端生成的版本 ID。
             */
            version_id: string;
            /** @description 加密版本元数据（hex）。 */
            version_meta: string;
            /** Format: int32 */
            chunk_count: number;
            /**
             * Format: int64
             * @description 密文字节总数（各块密文大小之和）。
             */
            declared_size: number;
        };
        /** @description 下载清单中的一块。 */
        DownloadChunk: {
            /** Format: int32 */
            idx: number;
            /**
             * Format: int64
             * @description 密文字节数（解密后应等于 4 MiB 或更小）。
             */
            ciphertext_size: number;
            /** @description 密文 SHA-256（hex），客户端逐块校验。 */
            sha256: string;
            method: string;
            /** @description 预签名 GET URL（≤ 15 分钟，含签名，禁止日志）。 */
            url: string;
        };
        /**
         * @description 下载清单。客户端 MUST 先解密 `encrypted_meta` 并校验其中的 content_id
         *     等于本响应的 `content_id`，再逐块解密（client-encryption spec）。
         */
        DownloadManifest: {
            /** Format: uuid */
            node_id: string;
            /** Format: uuid */
            version_id: string;
            /** Format: uuid */
            content_id: string;
            /** @description 加密版本元数据（hex）。 */
            encrypted_meta: string;
            /**
             * Format: int64
             * @description 该 content 的密文字节总数。
             */
            ciphertext_size: number;
            /** Format: int32 */
            chunk_count: number;
            /**
             * @description 文件节点的包裹密钥（hex）。客户端解包后用于解密版本元数据与块；
             *     刷新页面后无需先列目录也能下载。
             */
            wrapped_key?: string | null;
            /** @description 所有 URL 的过期时间（同一批次生成，≤ 15 分钟）。 */
            expires_at: string;
            chunks: components["schemas"]["DownloadChunk"][];
        };
        /** @description API 错误响应体（对外契约）。 */
        ErrorBody: {
            /** @description 稳定的机器可读错误码。 */
            code: string;
            /** @description 面向用户的通用文案。 */
            message: string;
            /** @description 服务端为本请求生成的 ID，便于运维检索日志。 */
            request_id: string;
            /** @description 仅 `upload_incomplete` 携带：尚未确认的块序号（升序）。 */
            missing_chunks?: number[] | null;
        };
        /** @description 存活检查响应。 */
        HealthResponse: {
            status: string;
        };
        /** @description KDF 参数（crypto-core 持久化 JSON 契约的 HTTP 表示）。 */
        KdfParams: {
            /** @description 算法标识；当前唯一允许 `argon2id`。 */
            algorithm: string;
            /**
             * Format: int32
             * @description 内存代价（KiB）。
             */
            m_cost_kib: number;
            /**
             * Format: int32
             * @description 迭代次数。
             */
            t_cost: number;
            /**
             * Format: int32
             * @description 并行度。
             */
            p_cost: number;
        };
        /** @description 列目录查询参数。 */
        ListChildrenQuery: {
            /** @description 上一页返回的 `next_cursor`；只编码排序键（节点 ID）。 */
            cursor?: string | null;
            /**
             * Format: int64
             * @description 页大小（1–1000，默认 200）。
             */
            limit?: number | null;
        };
        LoginRequest: {
            email: string;
            auth_key: string;
        };
        /** @description 登录成功响应：客户端需要 KDF 参数、盐与包裹后的 MK 来完成本地解锁。 */
        LoginResponse: {
            /** Format: uuid */
            user_id: string;
            email: string;
            email_verified: boolean;
            kdf_params: components["schemas"]["KdfParams"];
            kdf_salt: string;
            wrapped_mk: string;
            /**
             * Format: uuid
             * @description 当前用户的根文件夹 ID（非密钥；供文件浏览器导航使用）。
             */
            root_node_id: string;
            /** @description 账号状态（add-admin-console D2）。 */
            status: string;
            /** @description 停用/拒绝原因；其余状态为 null。 */
            status_reason?: string | null;
            /** @description 是否为管理员（只读）。 */
            is_admin: boolean;
        };
        /** @description 刷新页面后解锁所需的数据。 */
        MeResponse: {
            /** Format: uuid */
            user_id: string;
            email: string;
            email_verified: boolean;
            kdf_params: components["schemas"]["KdfParams"];
            kdf_salt: string;
            wrapped_mk: string;
            /**
             * Format: uuid
             * @description 当前用户的根文件夹 ID（非密钥；供文件浏览器导航使用）。
             */
            root_node_id: string;
            /** @description 账号状态（add-admin-console D2）。 */
            status: string;
            /** @description 停用/拒绝原因；其余状态为 null。 */
            status_reason?: string | null;
            /** @description 是否为管理员（只读）。 */
            is_admin: boolean;
        };
        /** @description 移动请求。名称令牌必须由客户端针对新父节点重新计算。 */
        MoveNodeRequest: {
            /** Format: uuid */
            new_parent_id: string;
            /** Format: int32 */
            row_version: number;
            name_token: string;
        };
        /** @description 节点摘要。所有二进制字段都用 hex 传输。 */
        NodeSummary: {
            /** Format: uuid */
            id: string;
            kind: string;
            /** Format: uuid */
            parent_id?: string | null;
            wrapped_key?: string | null;
            encrypted_meta?: string | null;
            name_token?: string | null;
            /** Format: int32 */
            row_version: number;
            current_version?: components["schemas"]["VersionSummary"] | null;
            created_at: string;
            updated_at: string;
            /** @description 回收站中的节点才非空。 */
            deleted_at?: string | null;
            /** Format: uuid */
            trash_batch_id?: string | null;
        };
        /** @description 通用成功响应。 */
        OkResponse: {
            ok: boolean;
        };
        /** @description 永久删除统计。 */
        PermanentDeleteResponse: {
            /** Format: int64 */
            deleted_nodes: number;
            /** Format: int64 */
            deleted_versions: number;
            /** Format: int64 */
            deleted_contents: number;
            /** Format: int64 */
            freed_bytes: number;
            /** Format: int64 */
            queued_objects: number;
        };
        /** @description `POST /api/v1/auth/prelogin`。 */
        PreloginRequest: {
            email: string;
        };
        /** @description prelogin 响应；不存在邮箱也返回同样的结构。 */
        PreloginResponse: {
            kdf_params: components["schemas"]["KdfParams"];
            kdf_salt: string;
        };
        /** @description 批量 presign 中的一项：成功时带 URL，失败时带 code/message。 */
        PresignChunkItemResult: {
            /** Format: int32 */
            idx: number;
            ok: boolean;
            url?: string | null;
            method?: string | null;
            expires_at?: string | null;
            /** Format: int64 */
            expected_size?: number | null;
            code?: string | null;
            message?: string | null;
        };
        /** @description presign 响应。`url` 含签名，客户端只能用于单次 PUT；服务端不记录日志。 */
        PresignChunkResponse: {
            /** Format: int32 */
            idx: number;
            method: string;
            url: string;
            /** @description 预签名 URL 的过期时间（不超过 15 分钟）。 */
            expires_at: string;
            /**
             * Format: int64
             * @description 该块的期望密文大小；客户端必须上传恰好这么多字节。
             */
            expected_size: number;
        };
        /** @description 批量 presign 请求。一次最多 [`MAX_BATCH_CHUNKS`] 个块。 */
        PresignChunksRequest: {
            indices: number[];
        };
        /** @description 批量 presign 响应。逐块独立结果，部分失败不影响其他块。 */
        PresignChunksResponse: {
            /** Format: uuid */
            session_id: string;
            items: components["schemas"]["PresignChunkItemResult"][];
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
        ReasonRequest: {
            reason: string;
        };
        RecoverRequest: {
            email: string;
            recovery_auth_key: string;
            new_auth_key: string;
            new_kdf_params: components["schemas"]["KdfParams"];
            new_kdf_salt: string;
            new_wrapped_mk: string;
        };
        /** @description 恢复重置前置查询：只返回恢复包裹后的 MK（密文）。 */
        RecoveryPreloginRequest: {
            email: string;
        };
        RecoveryPreloginResponse: {
            recovery_wrapped_mk: string;
        };
        /** @description 注册请求：只包含客户端派生的材料，绝不包含密码。 */
        RegisterRequest: {
            email: string;
            kdf_params: components["schemas"]["KdfParams"];
            kdf_salt: string;
            auth_key: string;
            wrapped_mk: string;
            recovery_wrapped_mk: string;
            recovery_auth_key: string;
        };
        /** @description 重命名请求。`row_version` 是乐观锁版本号。 */
        RenameNodeRequest: {
            /** Format: int32 */
            row_version: number;
            encrypted_meta: string;
            name_token: string;
        };
        ResendVerificationRequest: {
            email: string;
        };
        /**
         * @description 恢复请求。
         *
         *     不指定 `new_parent_id` 时恢复到原父节点并沿用原名称令牌；指定时必须同时
         *     提交客户端针对新父节点重新计算的 `name_token`。
         */
        RestoreNodeRequest: {
            /** Format: uuid */
            new_parent_id?: string | null;
            name_token?: string | null;
        };
        /** @description 恢复旧版本请求。 */
        RestoreVersionRequest: {
            /**
             * Format: uuid
             * @description 被恢复的历史版本 ID。
             */
            source_version_id: string;
            /**
             * Format: uuid
             * @description 客户端生成的新版本 ID（幂等键，必须为 UUIDv4）。
             */
            new_version_id: string;
            /** @description 客户端用同一 content_id 重新加密的版本元数据（hex）。 */
            encrypted_meta: string;
        };
        RotateRecoveryKeyRequest: {
            /** @description 当前密码派生出的认证密钥，用于证明知道当前密码。 */
            current_auth_key: string;
            new_recovery_wrapped_mk: string;
            new_recovery_auth_key: string;
        };
        /** @description 会话列表项。绝不返回令牌或令牌哈希。 */
        SessionSummary: {
            /** Format: uuid */
            id: string;
            created_at: string;
            last_seen_at: string;
            expires_at: string;
            ip: string;
            user_agent?: string | null;
            current: boolean;
        };
        SessionsResponse: {
            sessions: components["schemas"]["SessionSummary"][];
        };
        /** @description 软删除响应。重复删除同一节点返回同一个 `trash_batch_id`。 */
        TrashEntryResponse: {
            /** Format: uuid */
            node_id: string;
            /** Format: uuid */
            trash_batch_id: string;
        };
        /** @description 回收站列表参数。 */
        TrashQuery: {
            /** Format: int64 */
            limit?: number | null;
        };
        /** @description 回收站列表响应（只含每个 batch 的顶层项）。 */
        TrashResponse: {
            items: components["schemas"]["NodeSummary"][];
        };
        UpdateQuotaRequest: {
            /** Format: int64 */
            quota_bytes: number;
        };
        UpdateQuotaResponse: {
            ok: boolean;
            /** @description 新额度低于已用量时提示前端“该用户将无法继续上传”。 */
            below_used: boolean;
        };
        UpdateSettingsRequest: {
            registration_mode: string;
            /** Format: int64 */
            default_quota_bytes: number;
        };
        /** @description 上传会话摘要。 */
        UploadSessionResponse: {
            /** Format: uuid */
            id: string;
            /** Format: uuid */
            content_id: string;
            kind: string;
            status: string;
            /** Format: uuid */
            node_id: string;
            /** Format: uuid */
            parent_id?: string | null;
            /** Format: uuid */
            version_id: string;
            /** Format: int32 */
            chunk_count: number;
            /** Format: int64 */
            declared_size: number;
            /**
             * Format: int64
             * @description 已确认块数（续传时与 `GET .../chunks` 配合使用）。
             */
            confirmed_chunks: number;
            /**
             * @description 新文件：会话创建时客户端提交的包裹节点密钥（hex）。新版本：目标节点的
             *     包裹密钥。仅用于刷新后重新加载节点密钥，不含任何明文或密钥本身。
             */
            wrapped_key?: string | null;
            /** @description 新文件：加密节点元数据（hex）。新版本：目标节点的加密元数据。 */
            encrypted_meta?: string | null;
            expires_at: string;
            created_at: string;
        };
        /** @description 已用 / 预留 / 总额（字节）。 */
        UsageResponse: {
            /**
             * Format: int64
             * @description 所有保留版本（含回收站）引用 content 的密文字节数之和。
             */
            used_bytes: number;
            /**
             * Format: int64
             * @description 进行中上传会话预留的字节数（第 8 组使用）。
             */
            reserved_bytes: number;
            /**
             * Format: int64
             * @description 账号配额上限。
             */
            quota_bytes: number;
        };
        VerifyEmailRequest: {
            /** @description 前端从邮件链接的 `#token=` 片段读取后提交，绝不放在 URL 路径里。 */
            token: string;
        };
        /** @description 版本列表参数。 */
        VersionQuery: {
            /** Format: int64 */
            limit?: number | null;
        };
        /** @description 当前节点的版本摘要（当前版本与历史版本共用）。 */
        VersionSummary: {
            /** Format: uuid */
            id: string;
            /** Format: uuid */
            content_id: string;
            /** @description 版本元数据密文（hex）。 */
            encrypted_meta: string;
            /**
             * Format: int64
             * @description 该版本所引用 content 的密文字节数。
             */
            ciphertext_size: number;
            /** Format: int32 */
            chunk_count: number;
            is_current: boolean;
            created_at: string;
        };
        /** @description 版本列表响应（按创建时间倒序，含当前版本标记）。 */
        VersionsResponse: {
            items: components["schemas"]["VersionSummary"][];
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
    list_audit: {
        parameters: {
            query?: {
                /** @description 上一页的 next_cursor（审计 ID） */
                cursor?: string;
                /** @description 只看某个用户 */
                target_user_id?: string;
                /** @description 页大小 1–100，默认 50 */
                limit?: number;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AdminAuditResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    overview: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AdminOverview"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    get_settings: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AdminSettingsResponse"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    update_settings: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["UpdateSettingsRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AdminSettingsResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    list_users: {
        parameters: {
            query?: {
                /** @description pending/active/suspended/rejected */
                status?: string;
                /** @description 邮箱子串 */
                q?: string;
                /** @description 上一页的 next_cursor */
                cursor?: string;
                /** @description 页大小 1–100，默认 50 */
                limit?: number;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AdminUserListResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    get_user: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 用户 ID */
                user_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AdminUserDetailResponse"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    approve_user: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 用户 ID */
                user_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ApproveUserRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AdminOkResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    update_quota: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 用户 ID */
                user_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["UpdateQuotaRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["UpdateQuotaResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    reactivate_user: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 用户 ID */
                user_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AdminOkResponse"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    reject_user: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 用户 ID */
                user_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ReasonRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AdminOkResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    revoke_user_sessions: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 用户 ID */
                user_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AdminOkResponse"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    suspend_user: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 用户 ID */
                user_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ReasonRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AdminOkResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    change_password: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ChangePasswordRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["OkResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            429: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    login: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["LoginRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["LoginResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            429: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    logout: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["OkResponse"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    me: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["MeResponse"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    prelogin: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PreloginRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PreloginResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            429: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    recover: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["RecoverRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["OkResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            429: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    recovery_prelogin: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["RecoveryPreloginRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["RecoveryPreloginResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            429: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    register: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["RegisterRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["OkResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            429: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    resend_verification: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ResendVerificationRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["OkResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            429: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    rotate_recovery_key: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["RotateRecoveryKeyRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["OkResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            429: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    list_sessions: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SessionsResponse"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    revoke_session: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 要撤销的会话 ID */
                session_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["OkResponse"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    verify_email: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["VerifyEmailRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["OkResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    create_folder: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CreateFolderRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["NodeSummary"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    get_usage: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["UsageResponse"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    delete_node: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 节点 ID */
                node_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TrashEntryResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    list_children: {
        parameters: {
            query?: {
                /** @description 上一页的 next_cursor */
                cursor?: string;
                /** @description 页大小 1–1000，默认 200 */
                limit?: number;
            };
            header?: never;
            path: {
                /** @description 文件夹节点 ID */
                node_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ChildrenResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    move_node: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 节点 ID */
                node_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["MoveNodeRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["NodeSummary"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    rename_node: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 节点 ID */
                node_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["RenameNodeRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["NodeSummary"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    list_versions: {
        parameters: {
            query?: {
                /** @description 最多返回条数，默认 200 */
                limit?: number;
            };
            header?: never;
            path: {
                /** @description 文件节点 ID */
                node_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["VersionsResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    restore_version: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 文件节点 ID */
                node_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["RestoreVersionRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["VersionSummary"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    download_version: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 文件节点 ID */
                node_id: string;
                /** @description 版本 ID */
                version_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["DownloadManifest"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    list_trash: {
        parameters: {
            query?: {
                /** @description 最多返回条数，默认 200 */
                limit?: number;
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["TrashResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    permanently_delete_node: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 回收站顶层节点 ID */
                node_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PermanentDeleteResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    restore_node: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 回收站顶层节点 ID */
                node_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["RestoreNodeRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["NodeSummary"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    create_upload_session: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CreateUploadSessionRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["UploadSessionResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            429: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    get_upload_session: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 上传会话 ID */
                session_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["UploadSessionResponse"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    abort_upload: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 上传会话 ID */
                session_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["AbortUploadResponse"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            429: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    list_confirmed_chunks: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 上传会话 ID */
                session_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ConfirmedChunksResponse"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    confirm_chunks: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 上传会话 ID */
                session_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ConfirmChunksRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ConfirmChunksResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            429: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    presign_chunks: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 上传会话 ID */
                session_id: string;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PresignChunksRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PresignChunksResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            429: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    confirm_chunk: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 上传会话 ID */
                session_id: string;
                /** @description 块序号（从 0 开始） */
                idx: number;
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ConfirmChunkRequest"];
            };
        };
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ConfirmChunkResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            429: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    presign_chunk: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 上传会话 ID */
                session_id: string;
                /** @description 块序号（从 0 开始） */
                idx: number;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["PresignChunkResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            429: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
    commit_upload: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                /** @description 上传会话 ID */
                session_id: string;
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            200: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["CommitUploadResponse"];
                };
            };
            400: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            401: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            403: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            404: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            409: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            429: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
            500: {
                headers: {
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ErrorBody"];
                };
            };
        };
    };
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
