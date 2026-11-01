//! `desktop::update` 单元测试。
//!
//! 用本地 axum 假服务器与固定测试密钥覆盖：验签、防回滚、最低支持版本、
//! 不携带凭据、下载校验/取消/复用，以及 Windows 命令行与 macOS 辅助脚本的
//! 纯构造结果。测试不会运行安装包，也不会调用 hdiutil。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signer, SigningKey};

use crate::Clock;
use crate::release::{
    MANIFEST_SCHEMA, ReleaseArtifact, ReleaseHistoryEntry, ReleaseManifest, ReleasePlatforms,
};
use crate::state::StateStore;

use super::check::{HIGHEST_SEEN_KEY, UpdateCheck, UpdateUnavailableReason};
use super::download::{
    UpdateCancellation, UpdateDownloadStatus, UpdateDownloader, UpdatePlatform, UpdateProgress,
    UpdateProgressSink,
};
use super::install::{
    app_bundle_from_exe, macos_helper_script, macos_staging_path, windows_install_command,
};
use super::schedule::{CHECK_INTERVAL, STARTUP_DELAY, UpdateScheduler};
use super::{MAX_MANIFEST_BYTES, UpdateChecker, UpdateError};

const TEST_SEED: [u8; 32] = [0x42; 32];

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&TEST_SEED)
}

fn public_key() -> [u8; 32] {
    signing_key().verifying_key().to_bytes()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = crypto_core::ContentHasher::new();
    hasher.update(bytes);
    hasher.finish_hex()
}

fn sample_manifest(version: &str, min_supported: &str, package: &[u8]) -> ReleaseManifest {
    ReleaseManifest {
        schema: MANIFEST_SCHEMA,
        version: version.to_owned(),
        released_at: "2026-11-05T10:00:00Z".to_owned(),
        min_supported: min_supported.to_owned(),
        notes: vec!["支持微信备份".to_owned(), "托盘菜单修复".to_owned()],
        history: vec![ReleaseHistoryEntry {
            version: "0.1.0".to_owned(),
            released_at: "2026-11-01T10:00:00Z".to_owned(),
            notes: vec!["首个版本".to_owned()],
        }],
        platforms: ReleasePlatforms {
            windows_x64: ReleaseArtifact {
                file: format!("EncryptedBackup-{version}-windows-x64.exe"),
                size: package.len() as u64,
                sha256: sha256_hex(package),
            },
            macos_universal: ReleaseArtifact {
                file: format!("EncryptedBackup-{version}-macos.dmg"),
                size: package.len() as u64,
                sha256: sha256_hex(package),
            },
        },
    }
}

fn sign_manifest(manifest: &ReleaseManifest) -> (Vec<u8>, String) {
    let bytes = serde_json::to_vec_pretty(manifest).expect("序列化清单");
    let signature = signing_key().sign(&bytes);
    (bytes, BASE64.encode(signature.to_bytes()))
}

#[derive(Debug, Clone)]
struct RequestRecord {
    path: String,
    cookie: Option<String>,
    authorization: Option<String>,
    user_agent: Option<String>,
}

struct ServerState {
    manifest: Vec<u8>,
    signature: String,
    manifest_status: StatusCode,
    package: Vec<u8>,
    package_delay: Duration,
    requests: Vec<RequestRecord>,
    package_requests: usize,
}

impl ServerState {
    fn new(manifest: &ReleaseManifest, package: Vec<u8>) -> Self {
        let (bytes, signature) = sign_manifest(manifest);
        Self {
            manifest: bytes,
            signature,
            manifest_status: StatusCode::OK,
            package,
            package_delay: Duration::ZERO,
            requests: Vec::new(),
            package_requests: 0,
        }
    }

    fn set_manifest(&mut self, manifest: &ReleaseManifest) {
        let (bytes, signature) = sign_manifest(manifest);
        self.manifest = bytes;
        self.signature = signature;
    }
}

struct TestServer {
    base_url: String,
    state: Arc<Mutex<ServerState>>,
    handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    async fn start(state: ServerState) -> Self {
        let state = Arc::new(Mutex::new(state));
        let app = Router::new().fallback(any(serve)).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("绑定本地端口");
        let address = listener.local_addr().expect("读取本地地址");
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            base_url: format!("http://{address}"),
            state,
            handle,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, ServerState> {
        self.state.lock().expect("服务器状态锁")
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn serve(State(state): State<Arc<Mutex<ServerState>>>, request: Request) -> Response {
    let path = request.uri().path().to_owned();
    let record = RequestRecord {
        path: path.clone(),
        cookie: request
            .headers()
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
        authorization: request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
        user_agent: request
            .headers()
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
    };

    let (status, body, delay) = {
        let mut state = state.lock().expect("服务器状态锁");
        state.requests.push(record);
        match path.as_str() {
            "/downloads/latest.json" => (
                state.manifest_status,
                state.manifest.clone(),
                Duration::ZERO,
            ),
            "/downloads/latest.json.sig" => (
                StatusCode::OK,
                state.signature.clone().into_bytes(),
                Duration::ZERO,
            ),
            _ if path.starts_with("/downloads/") => {
                state.package_requests += 1;
                (StatusCode::OK, state.package.clone(), state.package_delay)
            }
            _ => (StatusCode::NOT_FOUND, Vec::new(), Duration::ZERO),
        }
    };
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    (status, body).into_response()
}

fn store() -> Arc<StateStore> {
    Arc::new(StateStore::open_in_memory().expect("内存状态库"))
}

fn checker(server: &TestServer, current: &str, store: Arc<StateStore>) -> UpdateChecker {
    UpdateChecker::new(&server.base_url, current, store)
        .expect("构造检查器")
        .with_public_key(public_key())
}

fn progress_sink() -> (Arc<Mutex<Vec<UpdateProgress>>>, UpdateProgressSink) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink: UpdateProgressSink = {
        let events = events.clone();
        Arc::new(move |event| events.lock().expect("进度锁").push(event))
    };
    (events, sink)
}

#[tokio::test]
async fn check_reports_available_newer_version() {
    let package = b"installer".to_vec();
    let manifest = sample_manifest("0.2.0", "0.1.0", &package);
    let server = TestServer::start(ServerState::new(&manifest, package)).await;
    let state = store();
    let outcome = checker(&server, "v0.1.0", state.clone()).check().await;

    match outcome.check {
        UpdateCheck::Available {
            manifest: found,
            forced,
        } => {
            assert_eq!(found.version, "0.2.0");
            assert_eq!(found.notes, manifest.notes);
            assert!(!forced);
        }
        other => panic!("应发现新版本，实际：{other:?}"),
    }
    assert!(outcome.checked_at > 0);
    assert_eq!(
        state
            .app_setting(HIGHEST_SEEN_KEY)
            .await
            .unwrap()
            .as_deref(),
        Some("0.2.0")
    );
    assert_eq!(server.state().package_requests, 0);
}

#[tokio::test]
async fn same_or_older_manifest_is_up_to_date() {
    let package = b"installer".to_vec();
    let manifest = sample_manifest("0.1.0", "0.1.0", &package);
    let server = TestServer::start(ServerState::new(&manifest, package)).await;
    let state = store();

    for current in ["0.1.0", "v0.1.0", "0.2.0"] {
        let outcome = checker(&server, current, state.clone()).check().await;
        assert_eq!(outcome.check, UpdateCheck::UpToDate, "{current}");
    }
}

#[tokio::test]
async fn development_build_is_reported_but_never_prompts() {
    let package = b"installer".to_vec();
    let manifest = sample_manifest("0.2.0", "0.1.0", &package);
    let server = TestServer::start(ServerState::new(&manifest, package)).await;
    let state = store();

    for current in ["c22934c", "v0.1.0-3-gabc1234", "dev"] {
        let outcome = checker(&server, current, state.clone()).check().await;
        assert_eq!(outcome.check, UpdateCheck::DevelopmentBuild, "{current}");
    }
}

#[tokio::test]
async fn tampered_manifest_is_unavailable_and_never_requests_package() {
    let package = b"installer".to_vec();
    let manifest = sample_manifest("0.2.0", "0.1.0", &package);
    let mut state = ServerState::new(&manifest, package);
    let middle = state.manifest.len() / 2;
    state.manifest[middle] ^= 0x01;
    let server = TestServer::start(state).await;

    let outcome = checker(&server, "v0.1.0", store()).check().await;
    assert_eq!(
        outcome.check,
        UpdateCheck::Unavailable {
            reason: UpdateUnavailableReason::InvalidSignature
        }
    );
    assert_eq!(server.state().package_requests, 0);
    assert_eq!(server.state().requests.len(), 2);
}

#[tokio::test]
async fn old_signed_manifest_cannot_downgrade() {
    let package = b"installer".to_vec();
    let newer = sample_manifest("0.3.0", "0.1.0", &package);
    let older = sample_manifest("0.2.0", "0.1.0", &package);
    let server = TestServer::start(ServerState::new(&newer, package.clone())).await;
    let state = store();
    let checker = checker(&server, "v0.1.0", state.clone());

    assert!(matches!(
        checker.check().await.check,
        UpdateCheck::Available { .. }
    ));
    server.state().set_manifest(&older);
    let outcome = checker.check().await;
    assert_eq!(
        outcome.check,
        UpdateCheck::Unavailable {
            reason: UpdateUnavailableReason::RollbackRejected
        }
    );
    assert_eq!(
        state
            .app_setting(HIGHEST_SEEN_KEY)
            .await
            .unwrap()
            .as_deref(),
        Some("0.3.0")
    );
    assert_eq!(server.state().package_requests, 0);
}

#[tokio::test]
async fn below_min_supported_sets_forced() {
    let package = b"installer".to_vec();
    let manifest = sample_manifest("0.2.0", "0.2.0", &package);
    let server = TestServer::start(ServerState::new(&manifest, package)).await;

    let outcome = checker(&server, "v0.1.0", store()).check().await;
    match outcome.check {
        UpdateCheck::Available { forced, .. } => assert!(forced),
        other => panic!("应为强制更新，实际：{other:?}"),
    }

    // 最低支持版本不高于当前版本时不强制。
    let unforced = sample_manifest("0.2.0", "0.1.0", b"installer");
    server.state().set_manifest(&unforced);
    let outcome = checker(&server, "v0.1.0", store()).check().await;
    match outcome.check {
        UpdateCheck::Available { forced, .. } => assert!(!forced),
        other => panic!("应为可选更新，实际：{other:?}"),
    }
}

#[test]
fn server_url_with_credentials_is_rejected() {
    let result = UpdateChecker::new("https://user:secret@example.com", "0.1.0", store());
    assert!(matches!(result, Err(UpdateError::InvalidServerUrl)));
}

#[tokio::test]
async fn check_requests_carry_no_credentials() {
    let package = b"installer".to_vec();
    let manifest = sample_manifest("0.2.0", "0.1.0", &package);
    let server = TestServer::start(ServerState::new(&manifest, package)).await;
    let _ = checker(&server, "v0.1.0", store()).check().await;

    let state = server.state();
    assert_eq!(state.requests.len(), 2);
    for request in &state.requests {
        assert!(request.cookie.is_none(), "{:?}", request.path);
        assert!(request.authorization.is_none(), "{:?}", request.path);
        let user_agent = request.user_agent.as_deref().expect("User-Agent");
        assert!(user_agent.starts_with("EncryptedBackup/"), "{user_agent}");
    }
}

#[tokio::test]
async fn manifest_body_over_limit_is_rejected() {
    let package = b"installer".to_vec();
    let manifest = sample_manifest("0.2.0", "0.1.0", &package);
    let mut state = ServerState::new(&manifest, package);
    state.manifest = vec![b'x'; MAX_MANIFEST_BYTES as usize + 1];
    let server = TestServer::start(state).await;

    let outcome = checker(&server, "v0.1.0", store()).check().await;
    assert_eq!(
        outcome.check,
        UpdateCheck::Unavailable {
            reason: UpdateUnavailableReason::InvalidManifest
        }
    );
    assert_eq!(server.state().package_requests, 0);
}

#[tokio::test]
async fn non_200_manifest_is_unavailable() {
    let package = b"installer".to_vec();
    let manifest = sample_manifest("0.2.0", "0.1.0", &package);
    let mut state = ServerState::new(&manifest, package);
    state.manifest_status = StatusCode::SERVICE_UNAVAILABLE;
    let server = TestServer::start(state).await;

    let outcome = checker(&server, "v0.1.0", store()).check().await;
    assert_eq!(
        outcome.check,
        UpdateCheck::Unavailable {
            reason: UpdateUnavailableReason::HttpStatus(503)
        }
    );
    assert_eq!(server.state().package_requests, 0);
}

#[tokio::test]
async fn banner_dismissal_roundtrip() {
    let package = b"installer".to_vec();
    let manifest = sample_manifest("0.2.0", "0.1.0", &package);
    let server = TestServer::start(ServerState::new(&manifest, package)).await;
    let checker = checker(&server, "v0.1.0", store());

    assert_eq!(checker.banner_dismissed_version().await.unwrap(), None);
    checker.dismiss_banner("0.2.0").await.unwrap();
    assert_eq!(
        checker.banner_dismissed_version().await.unwrap(),
        Some("0.2.0".to_owned())
    );
}

fn temp_downloader(server: &TestServer, temp: &tempfile::TempDir) -> UpdateDownloader {
    UpdateDownloader::new(&server.base_url, temp.path(), "0.1.0").expect("构造下载器")
}

#[tokio::test]
async fn download_verifies_size_and_sha256() {
    let package = b"the installer bytes".to_vec();
    let manifest = sample_manifest("0.2.0", "0.1.0", &package);
    let server = TestServer::start(ServerState::new(&manifest, package.clone())).await;
    let temp = tempfile::tempdir().expect("临时目录");
    let downloader = temp_downloader(&server, &temp);
    let (events, sink) = progress_sink();

    let verified = downloader
        .download_update(
            &manifest,
            UpdatePlatform::WindowsX64,
            &UpdateCancellation::new(),
            &sink,
        )
        .await
        .expect("下载并校验");

    assert_eq!(verified.size, package.len() as u64);
    assert_eq!(verified.sha256, sha256_hex(&package));
    assert_eq!(std::fs::read(&verified.path).unwrap(), package);
    assert!(
        verified
            .path
            .starts_with(temp.path().join("updates").join("0.2.0"))
    );
    let partial = temp
        .path()
        .join("updates")
        .join("0.2.0")
        .join(format!("{}.partial", manifest.platforms.windows_x64.file));
    assert!(!partial.exists());
    let events = events.lock().unwrap();
    let last = events.last().expect("进度事件");
    assert_eq!(last.status, UpdateDownloadStatus::Completed);
    assert_eq!(last.downloaded_bytes, package.len() as u64);
    assert_eq!(last.total_bytes, package.len() as u64);
    assert!(last.error.is_none());
}

#[tokio::test]
async fn download_rejects_tampered_package_and_deletes_files() {
    let package = b"the installer bytes".to_vec();
    let manifest = sample_manifest("0.2.0", "0.1.0", &package);
    let mut tampered = package.clone();
    let middle = tampered.len() / 2;
    tampered[middle] ^= 0x01;
    let server = TestServer::start(ServerState::new(&manifest, tampered)).await;
    let temp = tempfile::tempdir().expect("临时目录");
    let downloader = temp_downloader(&server, &temp);
    let (_, sink) = progress_sink();

    let error = downloader
        .download_update(
            &manifest,
            UpdatePlatform::WindowsX64,
            &UpdateCancellation::new(),
            &sink,
        )
        .await
        .expect_err("篡改必须失败");
    assert!(matches!(error, UpdateError::Tampered));

    let version_dir = temp.path().join("updates").join("0.2.0");
    let final_path = version_dir.join(&manifest.platforms.windows_x64.file);
    let partial = version_dir.join(format!("{}.partial", manifest.platforms.windows_x64.file));
    assert!(!final_path.exists());
    assert!(!partial.exists());
}

#[tokio::test]
async fn download_aborts_when_package_exceeds_manifest_size() {
    let package = b"the installer bytes".to_vec();
    let mut manifest = sample_manifest("0.2.0", "0.1.0", &package);
    manifest.platforms.windows_x64.size = (package.len() - 1) as u64;
    let server = TestServer::start(ServerState::new(&manifest, package)).await;
    let temp = tempfile::tempdir().expect("临时目录");
    let downloader = temp_downloader(&server, &temp);
    let (_, sink) = progress_sink();

    let error = downloader
        .download_update(
            &manifest,
            UpdatePlatform::WindowsX64,
            &UpdateCancellation::new(),
            &sink,
        )
        .await
        .expect_err("超长必须中止");
    assert!(matches!(error, UpdateError::Tampered));
    let version_dir = temp.path().join("updates").join("0.2.0");
    assert_eq!(std::fs::read_dir(&version_dir).unwrap().count(), 0);
}

#[tokio::test]
async fn cancel_removes_partial_file() {
    let package = vec![0x5a_u8; 1024 * 1024];
    let manifest = sample_manifest("0.2.0", "0.1.0", &package);
    let mut state = ServerState::new(&manifest, package);
    state.package_delay = Duration::from_secs(10);
    let server = TestServer::start(state).await;
    let temp = tempfile::tempdir().expect("临时目录");
    let downloader = temp_downloader(&server, &temp);
    let (_, sink) = progress_sink();
    let cancel = UpdateCancellation::new();
    let cancel_task = cancel.clone();
    let partial = temp
        .path()
        .join("updates")
        .join("0.2.0")
        .join(format!("{}.partial", manifest.platforms.windows_x64.file));

    let task = tokio::spawn(async move {
        downloader
            .download_update(&manifest, UpdatePlatform::WindowsX64, &cancel, &sink)
            .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel_task.cancel();

    let result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("取消必须在超时内完成")
        .expect("下载任务");
    assert!(matches!(result, Err(UpdateError::Cancelled)));
    assert!(!partial.exists());
}

#[tokio::test]
async fn verified_file_is_reused_without_requesting_again() {
    let package = b"the installer bytes".to_vec();
    let manifest = sample_manifest("0.2.0", "0.1.0", &package);
    let server = TestServer::start(ServerState::new(&manifest, package.clone())).await;
    let temp = tempfile::tempdir().expect("临时目录");
    let first = temp_downloader(&server, &temp)
        .download_update(
            &manifest,
            UpdatePlatform::WindowsX64,
            &UpdateCancellation::new(),
            &progress_sink().1,
        )
        .await
        .expect("首次下载");

    let requests_after_first = server.state().package_requests;
    // 让服务器内容失效；复用不应再请求安装包。
    server.state().package = b"completely different".to_vec();
    let second = temp_downloader(&server, &temp)
        .download_update(
            &manifest,
            UpdatePlatform::WindowsX64,
            &UpdateCancellation::new(),
            &progress_sink().1,
        )
        .await
        .expect("复用已校验文件");
    assert_eq!(second.path, first.path);
    assert_eq!(std::fs::read(&second.path).unwrap(), package);
    assert_eq!(server.state().package_requests, requests_after_first);
}

#[tokio::test]
async fn download_cleans_other_version_directories() {
    let package = b"the installer bytes".to_vec();
    let manifest = sample_manifest("0.2.0", "0.1.0", &package);
    let server = TestServer::start(ServerState::new(&manifest, package)).await;
    let temp = tempfile::tempdir().expect("临时目录");
    let old_version = temp.path().join("updates").join("0.1.0");
    std::fs::create_dir_all(&old_version).unwrap();
    std::fs::write(old_version.join("old.partial"), b"old").unwrap();

    let _ = temp_downloader(&server, &temp)
        .download_update(
            &manifest,
            UpdatePlatform::WindowsX64,
            &UpdateCancellation::new(),
            &progress_sink().1,
        )
        .await
        .expect("下载");
    assert!(!old_version.exists());
}

#[test]
fn windows_install_command_has_silent_update_flags() {
    let installer = Path::new(r"C:\Users\test\Downloads\EncryptedBackup-0.2.0-windows-x64.exe");
    let command = windows_install_command(installer);
    assert_eq!(command.program, PathBuf::from(installer));
    assert_eq!(
        command.args,
        vec![
            OsString::from("/SILENT"),
            OsString::from("/SUPPRESSMSGBOXES"),
            OsString::from("/NORESTART"),
            OsString::from("/CLOSEAPPLICATIONS"),
            OsString::from("/UPDATE=1"),
        ]
    );
}

#[test]
fn macos_helper_script_waits_replaces_and_opens() {
    let target = Path::new("/Applications/加密备份.app");
    let new = Path::new("/Applications/.加密备份.app.new");
    let old = Path::new("/Applications/.加密备份.app.old");
    let script = macos_helper_script(4242, target, new, old);

    assert!(script.contains("pid=4242"));
    assert!(script.contains("kill -0 \"$pid\""));
    assert!(script.contains("mv \"$target\" \"$old\""));
    assert!(script.contains("mv \"$new\" \"$target\""));
    assert!(script.contains("mv \"$old\" \"$target\""));
    assert!(script.contains("rm -rf \"$old\""));
    assert!(script.contains("open \"$target\""));
    assert!(script.contains("/Applications/加密备份.app"));
    assert!(script.contains("/Applications/.加密备份.app.new"));
    assert!(script.contains("/Applications/.加密备份.app.old"));
}

#[test]
fn macos_paths_are_derived_from_current_app() {
    assert_eq!(
        app_bundle_from_exe(Path::new(
            "/Applications/加密备份.app/Contents/MacOS/encrypted_backup_client"
        )),
        Some(PathBuf::from("/Applications/加密备份.app"))
    );
    assert_eq!(app_bundle_from_exe(Path::new("/usr/bin/tool")), None);
    assert_eq!(
        macos_staging_path(Path::new("/Applications/加密备份.app"), ".new"),
        Some(PathBuf::from("/Applications/.加密备份.app.new"))
    );
    assert_eq!(
        macos_staging_path(Path::new("/Applications/加密备份.app"), ".old"),
        Some(PathBuf::from("/Applications/.加密备份.app.old"))
    );
}

#[test]
fn scheduler_uses_startup_delay_and_daily_interval() {
    struct FakeClock {
        now: Mutex<SystemTime>,
        sleeps: Mutex<Vec<Duration>>,
    }

    impl Clock for FakeClock {
        fn sleep(&self, duration: Duration) -> crate::SleepFuture<'_> {
            self.sleeps.lock().unwrap().push(duration);
            Box::pin(async {})
        }

        fn now(&self) -> SystemTime {
            *self.now.lock().unwrap()
        }
    }

    let clock = Arc::new(FakeClock {
        now: Mutex::new(UNIX_EPOCH),
        sleeps: Mutex::new(Vec::new()),
    });
    let mut scheduler = UpdateScheduler::new(clock.clone());
    assert_eq!(scheduler.next_delay(), STARTUP_DELAY);

    scheduler.mark_checked();
    assert_eq!(scheduler.next_delay(), CHECK_INTERVAL);

    *clock.now.lock().unwrap() = UNIX_EPOCH + Duration::from_secs(3600);
    assert_eq!(
        scheduler.next_delay(),
        CHECK_INTERVAL - Duration::from_secs(3600)
    );

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(scheduler.wait_for_next());
    assert_eq!(
        clock.sleeps.lock().unwrap().clone(),
        vec![CHECK_INTERVAL - Duration::from_secs(3600)]
    );
    assert_eq!(scheduler.next_delay(), CHECK_INTERVAL);
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[test]
fn install_is_unsupported_on_other_platforms() {
    let verified = super::download::VerifiedUpdate {
        version: "0.2.0".to_owned(),
        platform: UpdatePlatform::WindowsX64,
        file_name: "installer.exe".to_owned(),
        path: PathBuf::from("installer.exe"),
        size: 1,
        sha256: "a".repeat(64),
    };
    assert!(matches!(
        super::install::install_update(&verified),
        Err(UpdateError::Unsupported)
    ));
    assert!(matches!(
        super::install::open_update_dmg(&verified),
        Err(UpdateError::Unsupported)
    ));
}
