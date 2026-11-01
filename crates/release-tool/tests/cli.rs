//! `release-tool` 集成测试：完整走一遍发布流程并在本地模拟篡改。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use client_core::release::ReleaseManifest;
use release_tool::{ToolError, run};
use tempfile::TempDir;

fn run_args(args: &[String]) -> Result<(), ToolError> {
    run(args.iter().cloned().map(OsString::from))
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).expect("写测试文件");
}

fn keygen(directory: &TempDir) -> (PathBuf, PathBuf) {
    let private = directory.path().join("release-signing.key");
    let public = directory.path().join("release-signing.pub");
    run_args(&[
        "keygen".to_owned(),
        "--out".to_owned(),
        path_text(&private),
        "--pub".to_owned(),
        path_text(&public),
    ])
    .expect("生成测试密钥");
    (private, public)
}

struct Fixture {
    directory: TempDir,
    private: PathBuf,
    public: PathBuf,
    artifacts: PathBuf,
    windows: PathBuf,
    macos: PathBuf,
    notes: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("临时目录");
        let (private, public) = keygen(&directory);
        let artifacts = directory.path().join("artifacts");
        fs::create_dir(&artifacts).expect("安装包目录");
        let windows = artifacts.join("EncryptedBackup-0.1.0-windows-x64.exe");
        let macos = artifacts.join("EncryptedBackup-0.1.0-macos.dmg");
        write(&windows, b"MZ fake windows installer v0.1.0");
        write(&macos, b"fake dmg payload v0.1.0");
        let notes = directory.path().join("notes.txt");
        write(&notes, "首个版本\n修复了托盘菜单\r\n".as_bytes());
        Self {
            directory,
            private,
            public,
            artifacts,
            windows,
            macos,
            notes,
        }
    }

    fn manifest(
        &self,
        version: &str,
        previous: Option<&Path>,
        out: &Path,
    ) -> Result<(), ToolError> {
        let mut args = strings(&[
            "manifest",
            "--version",
            version,
            "--released-at",
            "2026-11-01T10:00:00Z",
            "--min-supported",
            "0.1.0",
            "--notes",
        ]);
        args.push(path_text(&self.notes));
        args.extend(strings(&["--windows"]));
        args.push(path_text(&self.windows));
        args.extend(strings(&["--macos"]));
        args.push(path_text(&self.macos));
        if let Some(previous) = previous {
            args.push("--previous".to_owned());
            args.push(path_text(previous));
        }
        args.extend(strings(&["--out"]));
        args.push(path_text(out));
        run_args(&args)
    }

    fn sign(&self, input: &Path, out: &Path) -> Result<(), ToolError> {
        let mut args = strings(&["sign", "--key"]);
        args.push(path_text(&self.private));
        args.extend(strings(&["--in"]));
        args.push(path_text(input));
        args.extend(strings(&["--out"]));
        args.push(path_text(out));
        run_args(&args)
    }

    fn verify(&self, input: &Path, sig: &Path, files: Option<&Path>) -> Result<(), ToolError> {
        let mut args = strings(&["verify", "--pub"]);
        args.push(path_text(&self.public));
        args.extend(strings(&["--in"]));
        args.push(path_text(input));
        args.extend(strings(&["--sig"]));
        args.push(path_text(sig));
        if let Some(files) = files {
            args.push("--files".to_owned());
            args.push(path_text(files));
        }
        run_args(&args)
    }
}

#[test]
fn keygen_manifest_sign_verify_roundtrip_and_tamper_detection() {
    let fixture = Fixture::new();
    let latest = fixture.directory.path().join("latest.json");
    fixture.manifest("0.1.0", None, &latest).expect("生成清单");

    let text = fs::read_to_string(&latest).expect("读清单");
    assert!(text.ends_with('\n'), "清单必须以换行结尾");
    assert!(
        text.starts_with("{\n  \"schema\": 1,\n  \"version\": \"0.1.0\",\n"),
        "字段顺序与缩进必须稳定：{text}"
    );
    assert!(text.contains("\"windows-x64\""));
    assert!(text.contains("EncryptedBackup-0.1.0-windows-x64.exe"));
    assert!(text.contains("EncryptedBackup-0.1.0-macos.dmg"));

    let manifest: ReleaseManifest = serde_json::from_str(&text).expect("清单可严格解析");
    assert_eq!(manifest.version, "0.1.0");
    assert_eq!(manifest.notes, vec!["首个版本", "修复了托盘菜单"]);
    assert!(manifest.history.is_empty());
    assert_eq!(
        manifest.platforms.windows_x64.size,
        fs::metadata(&fixture.windows).unwrap().len()
    );
    assert_eq!(manifest.platforms.windows_x64.sha256.len(), 64);

    let sig = fixture.directory.path().join("latest.json.sig");
    fixture.sign(&latest, &sig).expect("签名");
    let sig_text = fs::read_to_string(&sig).expect("读签名");
    assert!(sig_text.ends_with('\n'));
    assert_eq!(sig_text.trim().lines().count(), 1, "签名必须只有一行");

    fixture
        .verify(&latest, &sig, Some(&fixture.artifacts))
        .expect("验签与安装包复核");

    // 清单改动 1 字节 → 验签失败。
    let original = fs::read(&latest).expect("读原始清单");
    let mut tampered = original.clone();
    let middle = tampered.len() / 2;
    tampered[middle] ^= 0x01;
    let tampered_manifest = fixture.directory.path().join("tampered.json");
    write(&tampered_manifest, &tampered);
    let error = fixture
        .verify(&tampered_manifest, &sig, None)
        .expect_err("篡改清单必须失败");
    assert!(error.to_string().contains("签名"), "{error}");
    assert_eq!(error.exit_code(), 1, "运行时失败退出码为 1");

    // 用另一把公钥验签 → 失败。
    let other = tempfile::tempdir().expect("另一把密钥");
    let (_other_private, other_public) = keygen(&other);
    let mut args = strings(&["verify", "--pub"]);
    args.push(path_text(&other_public));
    args.extend(strings(&["--in"]));
    args.push(path_text(&latest));
    args.extend(strings(&["--sig"]));
    args.push(path_text(&sig));
    assert!(
        run_args(&args)
            .expect_err("错误公钥必须失败")
            .to_string()
            .contains("签名")
    );

    // 安装包同长度改动 1 字节 → SHA-256 不一致。
    let original_macos = fs::read(&fixture.macos).expect("读 dmg");
    let mut tampered_macos = original_macos.clone();
    tampered_macos[0] ^= 0x01;
    write(&fixture.macos, &tampered_macos);
    let error = fixture
        .verify(&latest, &sig, Some(&fixture.artifacts))
        .expect_err("篡改安装包必须失败");
    assert!(error.to_string().contains("SHA-256"), "{error}");

    // 安装包长度变化 → 大小不一致。
    let mut longer = original_macos;
    longer.push(0x00);
    write(&fixture.macos, &longer);
    let error = fixture
        .verify(&latest, &sig, Some(&fixture.artifacts))
        .expect_err("安装包大小不一致必须失败");
    assert!(error.to_string().contains("大小"), "{error}");
}

#[test]
fn previous_manifest_history_is_inherited_and_version_must_increase() {
    let fixture = Fixture::new();
    let v1 = fixture.directory.path().join("latest-0.1.0.json");
    fixture.manifest("0.1.0", None, &v1).expect("v0.1.0");

    let v2 = fixture.directory.path().join("latest-0.2.0.json");
    fixture.manifest("0.2.0", Some(&v1), &v2).expect("v0.2.0");
    let manifest: ReleaseManifest =
        serde_json::from_slice(&fs::read(&v2).expect("读 v2")).expect("解析 v2");
    assert_eq!(manifest.history.len(), 1);
    assert_eq!(manifest.history[0].version, "0.1.0");
    assert_eq!(
        manifest.history[0].notes,
        vec!["首个版本", "修复了托盘菜单"]
    );
    assert_eq!(manifest.history[0].released_at, "2026-11-01T10:00:00Z");

    let v3 = fixture.directory.path().join("latest-0.3.0.json");
    fixture.manifest("0.3.0", Some(&v2), &v3).expect("v0.3.0");
    let manifest: ReleaseManifest =
        serde_json::from_slice(&fs::read(&v3).expect("读 v3")).expect("解析 v3");
    assert_eq!(manifest.history.len(), 2);
    assert_eq!(manifest.history[0].version, "0.2.0");
    assert_eq!(manifest.history[1].version, "0.1.0");

    let error = fixture
        .manifest(
            "0.2.0",
            Some(&v3),
            &fixture.directory.path().join("bad.json"),
        )
        .expect_err("同版本不能再发布");
    assert!(error.to_string().contains("必须高于"), "{error}");
}

#[test]
fn keygen_refuses_to_overwrite_existing_private_key() {
    let fixture = Fixture::new();
    let before = fs::read(&fixture.private).expect("读私钥");
    assert!(!before.is_empty());

    let error = run_args(&[
        "keygen".to_owned(),
        "--out".to_owned(),
        path_text(&fixture.private),
        "--pub".to_owned(),
        path_text(&fixture.public),
    ])
    .expect_err("已有私钥必须拒绝覆盖");
    assert!(error.to_string().contains("拒绝覆盖"), "{error}");
    assert_eq!(error.exit_code(), 1);
    assert_eq!(fs::read(&fixture.private).expect("读私钥"), before);

    // 只存在公钥时同样拒绝，避免留下无人能用的新私钥。
    let directory = tempfile::tempdir().expect("临时目录");
    let private = directory.path().join("new.key");
    let public = directory.path().join("existing.pub");
    write(&public, b"existing");
    let error = run_args(&[
        "keygen".to_owned(),
        "--out".to_owned(),
        path_text(&private),
        "--pub".to_owned(),
        path_text(&public),
    ])
    .expect_err("已有公钥必须拒绝覆盖");
    assert!(error.to_string().contains("拒绝覆盖"), "{error}");
    assert!(!private.exists(), "失败时不应留下新私钥");
}

#[test]
fn manifest_rejects_bad_arguments() {
    let fixture = Fixture::new();
    let out = fixture.directory.path().join("latest.json");

    let error = fixture
        .manifest("0.2.0-dev", None, &out)
        .expect_err("预发布版本");
    assert!(error.to_string().contains("语义化版本"), "{error}");
    assert_eq!(error.exit_code(), 2, "参数错误退出码为 2");

    let mut args = strings(&[
        "manifest",
        "--version",
        "0.2.0",
        "--released-at",
        "2026-13-01T10:00:00Z",
        "--min-supported",
        "0.1.0",
        "--notes",
    ]);
    args.push(path_text(&fixture.notes));
    args.extend(strings(&["--windows"]));
    args.push(path_text(&fixture.windows));
    args.extend(strings(&["--macos"]));
    args.push(path_text(&fixture.macos));
    args.extend(strings(&["--out"]));
    args.push(path_text(&out));
    let error = run_args(&args).expect_err("非法日期");
    assert!(error.to_string().contains("RFC3339"), "{error}");

    // 安装包不存在。
    let mut args = strings(&[
        "manifest",
        "--version",
        "0.2.0",
        "--released-at",
        "2026-11-01T10:00:00Z",
        "--min-supported",
        "0.1.0",
        "--notes",
    ]);
    args.push(path_text(&fixture.notes));
    args.extend(strings(&["--windows"]));
    args.push(path_text(&fixture.directory.path().join("missing.exe")));
    args.extend(strings(&["--macos"]));
    args.push(path_text(&fixture.macos));
    args.extend(strings(&["--out"]));
    args.push(path_text(&out));
    let error = run_args(&args).expect_err("缺少安装包");
    assert!(error.to_string().contains("无法打开安装包"), "{error}");
}
