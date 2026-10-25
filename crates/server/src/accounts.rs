//! 账号状态常量（add-admin-console design D1/D2）。
//!
//! 状态只允许四个取值，与数据库 CHECK 约束一致；服务端一律以字符串比较，
//! 避免 JSON/HTTP 层出现第二套状态枚举。

pub const PENDING: &str = "pending";
pub const ACTIVE: &str = "active";
pub const SUSPENDED: &str = "suspended";
pub const REJECTED: &str = "rejected";

/// 是否为合法的账号状态。
pub fn is_valid_status(status: &str) -> bool {
    matches!(status, PENDING | ACTIVE | SUSPENDED | REJECTED)
}

/// 是否允许使用服务（读取之外的写操作）。
pub fn is_active(status: &str) -> bool {
    status == ACTIVE
}
