//! 微信账号本地备注名（design D1/D5）。
//!
//! 备注名只保存在本地状态库 `wechat_accounts` 表中，键是
//! 数据根目录 + 账号目录名；不写云端、不包含任何聊天内容。

use std::path::Path;

use crate::state::{StateError, StateStore};

use super::WechatError;

/// 备注名最大字符数（Unicode 标量值，不是字节数）。
pub const WECHAT_NOTE_MAX_CHARS: usize = 64;

/// 微信账号备注名的读写入口。
#[derive(Clone)]
pub struct WechatAccountNotes {
    state: StateStore,
}

impl WechatAccountNotes {
    /// 包装已有的本地状态库。
    #[must_use]
    pub fn new(state: StateStore) -> Self {
        Self { state }
    }

    /// 读取备注名；没有设置时返回 `None`。
    pub async fn get(
        &self,
        data_root: &Path,
        account_name: &str,
    ) -> Result<Option<String>, WechatError> {
        let (data_root, account_name) = note_key(data_root, account_name)?;
        Ok(self
            .state
            .wechat_account_note(data_root, account_name)
            .await?)
    }

    /// 设置备注名（覆盖旧值）。空白、控制字符与超长值会被拒绝。
    pub async fn set(
        &self,
        data_root: &Path,
        account_name: &str,
        note: &str,
    ) -> Result<(), WechatError> {
        let (data_root, account_name) = note_key(data_root, account_name)?;
        let note = validate_note(note)?;
        self.state
            .set_wechat_account_note(data_root, account_name, note)
            .await?;
        Ok(())
    }

    /// 清除备注名；本来没有备注时也返回成功。
    pub async fn clear(&self, data_root: &Path, account_name: &str) -> Result<(), WechatError> {
        let (data_root, account_name) = note_key(data_root, account_name)?;
        self.state
            .clear_wechat_account_note(data_root, account_name)
            .await?;
        Ok(())
    }
}

fn note_key(data_root: &Path, account_name: &str) -> Result<(String, String), WechatError> {
    let data_root = data_root
        .to_str()
        .ok_or(StateError::PathEncoding)?
        .to_owned();
    if account_name.is_empty()
        || account_name == "."
        || account_name == ".."
        || account_name.contains('/')
        || account_name.contains('\\')
        || account_name.chars().any(char::is_control)
    {
        return Err(WechatError::InvalidAccount);
    }
    Ok((data_root, account_name.to_owned()))
}

fn validate_note(note: &str) -> Result<String, WechatError> {
    let trimmed = note.trim();
    if trimmed.is_empty() {
        return Err(WechatError::InvalidNote("备注名不能为空"));
    }
    if trimmed.chars().count() > WECHAT_NOTE_MAX_CHARS {
        return Err(WechatError::InvalidNote("备注名过长"));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(WechatError::InvalidNote("备注名不能包含控制字符"));
    }
    Ok(trimmed.to_owned())
}
