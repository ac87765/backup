-- 0006：账号审批、注册设置、管理员与审计（add-admin-console design D1）。
--
-- 只增不改：
-- - 新增列都有默认值，旧版本代码可以在新 schema 上继续运行；
-- - 迁移把**已存在**的用户全部设为 active，保证升级后行为不变；
-- - 新注册用户的默认状态由应用按注册模式显式写入（默认 pending）。
--
-- is_admin 只能由 `server admin grant|revoke` 修改；HTTP 层没有任何写路径。

ALTER TABLE users
    ADD COLUMN status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'active', 'suspended', 'rejected')),
    ADD COLUMN is_admin BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN status_changed_at TIMESTAMPTZ,
    ADD COLUMN status_reason TEXT
        CHECK (status_reason IS NULL OR char_length(status_reason) <= 500),
    ADD COLUMN approved_at TIMESTAMPTZ,
    ADD COLUMN approved_by UUID REFERENCES users (id) ON DELETE SET NULL;

-- 迁移前已存在的账号一律保持可用，不需要补审批。
UPDATE users
SET status = 'active',
    status_changed_at = now()
WHERE status = 'pending';

CREATE INDEX users_status_created_idx ON users (status, created_at DESC);

-- 应用设置：registration_mode（approval/open/closed）与 default_quota_bytes。
-- default_quota_bytes 行可以不存在：读取时回退到 BACKUP_DEFAULT_QUOTA_BYTES，
-- 从而让配置项在尚未修改过设置时仍然生效。
CREATE TABLE app_settings (
    key TEXT PRIMARY KEY,
    value JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by UUID REFERENCES users (id) ON DELETE SET NULL
);

INSERT INTO app_settings (key, value)
VALUES ('registration_mode', '"approval"'::jsonb);

-- 管理审计：只追加；admin_id 不使用外键，以便命令行操作使用 nil 哨兵值，
-- 且用户被删除后审计记录仍然保留。不保存令牌、密钥或加密元数据。
CREATE TABLE admin_audit_log (
    id BIGSERIAL PRIMARY KEY,
    admin_id UUID NOT NULL,
    action TEXT NOT NULL,
    target_user_id UUID,
    details JSONB NOT NULL DEFAULT '{}'::jsonb,
    ip TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX admin_audit_log_created_idx ON admin_audit_log (created_at DESC, id DESC);
CREATE INDEX admin_audit_log_target_idx
    ON admin_audit_log (target_user_id, created_at DESC, id DESC);
