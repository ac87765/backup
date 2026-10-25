/**
 * Worker 暴露给主线程的完整 API：加密方法 + 认证高层方法。
 *
 * 认证方法内部复用同一个 KeyStore（`CryptoApi` 与 `AuthClient` 共享同一个后端 Promise），
 * 因此 prepare/commit 的状态变化对两侧一致。
 */
import { AuthClient } from './auth-client'
import { CryptoApi } from './crypto-api'
import { FileClient } from './file-client'
import { TransferClient } from './transfer-client'
import type {
  AuthErrorPayload,
  AuthWorkerApi,
  CryptoBackend,
  CurrentUser,
  DeleteResultView,
  DownloadResultView,
  DownloadTargetView,
  FileNodeView,
  FileWorkerApi,
  FolderPageView,
  PendingUploadView,
  SessionSummaryView,
  TransferProgressView,
  TransferWorkerApi,
  TrashNodeView,
  UploadResultView,
  UsageView,
  VersionView,
} from './types'

/** 把 WASM/网络错误统一成可跨 Comlink 的普通对象（保留 code/retryAfter）。 */
function toAuthPayload(error: unknown): AuthErrorPayload {
  const candidate = error as
    | { code?: unknown; message?: unknown; retryAfter?: unknown }
    | null
  return {
    code:
      candidate && typeof candidate.code === 'string'
        ? candidate.code
        : 'crypto_error',
    message:
      candidate && typeof candidate.message === 'string'
        ? candidate.message
        : '认证操作失败',
    ...(candidate && typeof candidate.retryAfter === 'number'
      ? { retryAfter: candidate.retryAfter }
      : {}),
  }
}

async function runApi<T>(operation: () => Promise<T>): Promise<T> {
  try {
    return await operation()
  } catch (error) {
    // Comlink 只可靠传输结构化克隆数据；Error 上的自定义 code 会丢失。
    // eslint-disable-next-line @typescript-eslint/only-throw-error
    throw toAuthPayload(error)
  }
}

export class WorkerApi
  extends CryptoApi
  implements AuthWorkerApi, FileWorkerApi, TransferWorkerApi
{
  private readonly auth: AuthClient
  private readonly file: FileClient
  private readonly transfer: TransferClient

  constructor(backend: CryptoBackend | Promise<CryptoBackend>) {
    const promise = Promise.resolve(backend)
    super(promise)
    this.auth = new AuthClient(promise)
    this.file = new FileClient(promise)
    this.transfer = new TransferClient(promise, this.file)
  }

  register(
    email: string,
    password: string,
  ): Promise<{ recoveryDisplay: string }> {
    return runApi(() => this.auth.register(email, password))
  }

  confirmRegistration(): Promise<void> {
    return runApi(() => this.auth.confirmRegistration())
  }

  discardRegistration(): Promise<void> {
    return runApi(() => this.auth.discardRegistration())
  }

  validateRecoveryKey(display: string): Promise<void> {
    return runApi(() => this.auth.validateRecoveryKey(display))
  }

  login(email: string, password: string): Promise<CurrentUser> {
    return runApi(() => this.auth.login(email, password))
  }

  refreshSession(): Promise<CurrentUser | null> {
    return runApi(() => this.auth.refreshSession())
  }

  unlock(password: string): Promise<void> {
    return runApi(() => this.auth.unlock(password))
  }

  changePassword(oldPassword: string, newPassword: string): Promise<void> {
    return runApi(() => this.auth.changePassword(oldPassword, newPassword))
  }

  resetWithRecovery(
    email: string,
    recoveryKey: string,
    newPassword: string,
  ): Promise<void> {
    return runApi(() =>
      this.auth.resetWithRecovery(email, recoveryKey, newPassword),
    )
  }

  prepareRecoveryRotation(
    currentPassword: string,
  ): Promise<{ recoveryDisplay: string }> {
    return runApi(() => this.auth.prepareRecoveryRotation(currentPassword))
  }

  confirmRecoveryRotation(): Promise<void> {
    return runApi(() => this.auth.confirmRecoveryRotation())
  }

  discardRecoveryRotation(): Promise<void> {
    return runApi(() => this.auth.discardRecoveryRotation())
  }

  logout(): Promise<void> {
    return runApi(() => this.auth.logout())
  }

  verifyEmail(token: string): Promise<void> {
    return runApi(() => this.auth.verifyEmail(token))
  }

  resendVerification(email: string): Promise<void> {
    return runApi(() => this.auth.resendVerification(email))
  }

  getSessions(): Promise<SessionSummaryView[]> {
    return runApi(() => this.auth.getSessions())
  }

  revokeSession(sessionId: string): Promise<void> {
    return runApi(() => this.auth.revokeSession(sessionId))
  }

  listFolder(nodeId: string, cursor?: string): Promise<FolderPageView> {
    return runApi(() => this.file.listFolder(nodeId, cursor))
  }

  createFolder(parentId: string, name: string): Promise<FileNodeView> {
    return runApi(() => this.file.createFolder(parentId, name))
  }

  renameNode(
    nodeId: string,
    parentId: string,
    name: string,
    rowVersion: number,
  ): Promise<FileNodeView> {
    return runApi(() =>
      this.file.renameNode(nodeId, parentId, name, rowVersion),
    )
  }

  moveNode(
    nodeId: string,
    newParentId: string,
    name: string,
    rowVersion: number,
  ): Promise<FileNodeView> {
    return runApi(() =>
      this.file.moveNode(nodeId, newParentId, name, rowVersion),
    )
  }

  deleteNode(nodeId: string): Promise<void> {
    return runApi(() => this.file.deleteNode(nodeId))
  }

  listTrash(): Promise<TrashNodeView[]> {
    return runApi(() => this.file.listTrash())
  }

  restoreNode(
    nodeId: string,
    newParentId?: string,
    name?: string,
  ): Promise<FileNodeView> {
    return runApi(() => this.file.restoreNode(nodeId, newParentId, name))
  }

  permanentlyDeleteNode(nodeId: string): Promise<DeleteResultView> {
    return runApi(() => this.file.permanentlyDeleteNode(nodeId))
  }

  listVersions(nodeId: string): Promise<VersionView[]> {
    return runApi(() => this.file.listVersions(nodeId))
  }

  restoreVersion(nodeId: string, sourceVersionId: string): Promise<VersionView> {
    return runApi(() => this.file.restoreVersion(nodeId, sourceVersionId))
  }

  getUsage(): Promise<UsageView> {
    return runApi(() => this.file.getUsage())
  }

  uploadFile(
    parentId: string,
    file: File,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<UploadResultView> {
    return runApi(() => this.transfer.uploadFile(parentId, file, onProgress))
  }

  uploadNewVersion(
    nodeId: string,
    file: File,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<UploadResultView> {
    return runApi(() =>
      this.transfer.uploadNewVersion(nodeId, file, onProgress),
    )
  }

  resumePendingUpload(
    sessionId: string,
    file: File,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<UploadResultView> {
    return runApi(() =>
      this.transfer.resumePendingUpload(sessionId, file, onProgress),
    )
  }

  listPendingUploads(): Promise<PendingUploadView[]> {
    return runApi(() => this.transfer.listPendingUploads())
  }

  pauseUpload(sessionId: string): Promise<void> {
    return runApi(() => this.transfer.pauseUpload(sessionId))
  }

  resumeUpload(sessionId: string): Promise<void> {
    return runApi(() => this.transfer.resumeUpload(sessionId))
  }

  cancelUpload(sessionId: string): Promise<void> {
    return runApi(() => this.transfer.cancelUpload(sessionId))
  }

  downloadVersion(
    nodeId: string,
    versionId: string,
    downloadId: string,
    fileName: string,
    target: DownloadTargetView,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<DownloadResultView> {
    return runApi(() =>
      this.transfer.downloadVersion(
        nodeId,
        versionId,
        downloadId,
        fileName,
        target,
        onProgress,
      ),
    )
  }

  pauseDownload(downloadId: string): Promise<void> {
    return runApi(() => this.transfer.pauseDownload(downloadId))
  }

  resumeDownload(downloadId: string): Promise<void> {
    return runApi(() => this.transfer.resumeDownload(downloadId))
  }

  cancelDownload(downloadId: string): Promise<void> {
    return runApi(() => this.transfer.cancelDownload(downloadId))
  }
}
