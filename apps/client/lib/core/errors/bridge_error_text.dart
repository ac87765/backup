import '../rust/models.dart';

/// 把桥接错误映射成面向用户的中文提示。
///
/// 优先使用 Rust 给出的 `BridgeError.message`（已与 client-core / 服务端文案
/// 一致）；只有需要统一措辞或补充上下文的错误码才在这里覆盖。不记录、不展示
/// 任何密钥、令牌、路径或请求体。
String bridgeErrorText(Object error) {
  if (error is! BridgeError) {
    return '操作失败，请重试';
  }
  switch (error.code) {
    case 'invalid_credentials':
      // 与 Web 及服务端一致：不区分账号是否存在。
      return '邮箱或密码不正确';
    case 'unlock_failed':
      return '密码不正确';
    case 'account_pending':
      return '账号正在等待管理员审核';
    case 'account_suspended':
      return '账号已被停用，请联系管理员';
    case 'account_rejected':
      return '账号未通过审核，请联系管理员';
    case 'account_temporarily_locked':
      return '尝试次数过多，请稍后再试';
    case 'network_error':
      return '无法连接服务器，请检查网络后重试';
    case 'rate_limited':
      return '服务器繁忙，请稍后重试';
    case 'quota_exceeded':
      return '存储空间不足';
    case 'name_conflict':
      return '当前文件夹已有同名项目';
    case 'target_unavailable':
      return '原位置已不可用，请选择其他文件夹';
    case 'account_not_writable':
      return '账号通过审核后才能上传和备份文件';
    case 'locked':
      return '需要解锁后才能继续';
    case 'file_changed':
      return '文件在上传过程中被修改，稍后会重试';
    case 'integrity_error':
      return '文件校验失败，已停止本次传输';
    case 'wechat_running':
      return '请先完全退出微信（包括托盘图标），再开始恢复';
    case 'snapshot_incomplete':
      return '所选数据库快照不完整，不能用于恢复';
    case 'snapshot_corrupted':
      return '快照内容与清单不一致，恢复失败，原数据库已保留';
    case 'snapshot_not_found':
      return '找不到所选的数据库快照';
  }
  return error.message.isEmpty ? '操作失败，请重试' : error.message;
}

/// 是否为“登录凭据错误”以外的网络类错误（用于是否保留表单输入）。
bool isNetworkError(Object error) =>
    error is BridgeError && error.code == 'network_error';
