//! `release` 模块单元测试。
//!
//! 测试密钥用固定种子在测试内生成；生产公钥由 `RELEASE_PUBLIC_KEY` 常量提供，
//! 这里只验证常量与 `deploy/release-signing.pub` 一致。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signer, SigningKey};

use super::*;

/// 测试专用固定种子；绝不能用于真实发布。
const TEST_SEED: [u8; 32] = [0x42; 32];

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&TEST_SEED)
}

fn sample_manifest() -> ReleaseManifest {
    ReleaseManifest {
        schema: MANIFEST_SCHEMA,
        version: "0.2.0".to_owned(),
        released_at: "2026-11-05T10:00:00Z".to_owned(),
        min_supported: "0.1.0".to_owned(),
        notes: vec!["支持微信备份".to_owned(), "托盘菜单修复".to_owned()],
        history: vec![ReleaseHistoryEntry {
            version: "0.1.0".to_owned(),
            released_at: "2026-11-01T10:00:00Z".to_owned(),
            notes: vec!["首个版本".to_owned()],
        }],
        platforms: ReleasePlatforms {
            windows_x64: ReleaseArtifact {
                file: "EncryptedBackup-0.2.0-windows-x64.exe".to_owned(),
                size: 15_400_000,
                sha256: "a".repeat(64),
            },
            macos_universal: ReleaseArtifact {
                file: "EncryptedBackup-0.2.0-macos.dmg".to_owned(),
                size: 38_500_000,
                sha256: "b".repeat(64),
            },
        },
    }
}

fn sign_bytes(bytes: &[u8]) -> (String, [u8; 32]) {
    let key = signing_key();
    let signature = key.sign(bytes);
    (
        BASE64.encode(signature.to_bytes()),
        key.verifying_key().to_bytes(),
    )
}

fn signed(manifest: &ReleaseManifest) -> (Vec<u8>, String, [u8; 32]) {
    let bytes = serde_json::to_vec_pretty(manifest).expect("序列化清单");
    let (sig, public_key) = sign_bytes(&bytes);
    (bytes, sig, public_key)
}

#[test]
fn signature_round_trip() {
    let manifest = sample_manifest();
    let (bytes, sig, public_key) = signed(&manifest);
    let parsed = verify_manifest(&bytes, &sig, &public_key).expect("验签并解析");
    assert_eq!(parsed, manifest);
}

#[test]
fn one_byte_manifest_change_fails_signature() {
    let manifest = sample_manifest();
    let (mut bytes, sig, public_key) = signed(&manifest);
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0x01;
    let error = verify_manifest(&bytes, &sig, &public_key).expect_err("篡改后必须失败");
    assert!(matches!(error, ReleaseError::InvalidSignature));
}

#[test]
fn invalid_signature_encodings_are_rejected() {
    let manifest = sample_manifest();
    let (bytes, _, public_key) = signed(&manifest);

    let error = verify_manifest(&bytes, "不是 base64", &public_key).expect_err("非法 Base64");
    assert!(matches!(error, ReleaseError::InvalidSignatureEncoding));

    let short = BASE64.encode([0u8; 3]);
    let error = verify_manifest(&bytes, &short, &public_key).expect_err("长度错误");
    assert!(matches!(error, ReleaseError::InvalidSignatureLength));

    let wrong = BASE64.encode([0u8; 64]);
    let error = verify_manifest(&bytes, &wrong, &public_key).expect_err("签名错误");
    assert!(matches!(error, ReleaseError::InvalidSignature));
}

#[test]
fn signature_from_another_key_is_rejected() {
    let manifest = sample_manifest();
    let (bytes, sig, _) = signed(&manifest);
    let other = SigningKey::from_bytes(&[0x24; 32]);
    let error = verify_manifest(&bytes, &sig, &other.verifying_key().to_bytes())
        .expect_err("不同公钥必须失败");
    assert!(matches!(error, ReleaseError::InvalidSignature));
}

#[test]
fn signature_is_checked_before_json_parsing() {
    let manifest = sample_manifest();
    let (_, sig, public_key) = signed(&manifest);
    let error = verify_manifest(b"{not json", &sig, &public_key).expect_err("非法 JSON");
    // 先验签：原始字节被改动，因此是签名失败，而不是 JSON 解析错误。
    assert!(matches!(error, ReleaseError::InvalidSignature));
}

#[test]
fn unknown_schema_is_rejected() {
    let mut manifest = sample_manifest();
    manifest.schema = 2;
    let (bytes, sig, public_key) = signed(&manifest);
    let error = verify_manifest(&bytes, &sig, &public_key).expect_err("未知 schema");
    assert!(matches!(error, ReleaseError::UnsupportedSchema(2)));

    assert!(serde_json::from_slice::<ReleaseManifest>(&bytes).is_err());
}

#[test]
fn unknown_fields_are_rejected() {
    let manifest = sample_manifest();
    let mut value = serde_json::to_value(&manifest).expect("转 JSON");
    value
        .as_object_mut()
        .expect("对象")
        .insert("future_field".to_owned(), serde_json::json!(true));
    let bytes = serde_json::to_vec_pretty(&value).expect("序列化");
    let (sig, public_key) = sign_bytes(&bytes);
    let error = verify_manifest(&bytes, &sig, &public_key).expect_err("未知字段");
    assert!(matches!(error, ReleaseError::InvalidManifest(_)));

    let mut value = serde_json::to_value(&manifest).expect("转 JSON");
    let platforms = value
        .get_mut("platforms")
        .and_then(serde_json::Value::as_object_mut)
        .expect("platforms");
    platforms.insert(
        "linux-x64".to_owned(),
        serde_json::json!({"file": "x", "size": 1, "sha256": "a".repeat(64)}),
    );
    let bytes = serde_json::to_vec_pretty(&value).expect("序列化");
    let (sig, public_key) = sign_bytes(&bytes);
    let error = verify_manifest(&bytes, &sig, &public_key).expect_err("未知平台");
    assert!(matches!(error, ReleaseError::InvalidManifest(_)));
}

#[test]
fn invalid_file_names_are_rejected() {
    for bad in [
        "a/b.exe",
        "a\\b.exe",
        "..\\x.exe",
        "a..b.exe",
        "C:x.exe",
        "a.exe:stream",
        "bad\nname.exe",
        "",
    ] {
        let mut manifest = sample_manifest();
        manifest.platforms.windows_x64.file = bad.to_owned();
        let (bytes, sig, public_key) = signed(&manifest);
        let error = verify_manifest(&bytes, &sig, &public_key).expect_err("非法文件名");
        assert!(
            matches!(error, ReleaseError::InvalidFileName),
            "文件名 {bad:?} 应被拒绝"
        );
    }
}

#[test]
fn invalid_sha256_is_rejected() {
    for bad in [
        "A".repeat(64),
        "a".repeat(63),
        "g".repeat(64),
        String::new(),
    ] {
        let mut manifest = sample_manifest();
        manifest.platforms.macos_universal.sha256 = bad;
        let (bytes, sig, public_key) = signed(&manifest);
        let error = verify_manifest(&bytes, &sig, &public_key).expect_err("非法 SHA-256");
        assert!(matches!(error, ReleaseError::InvalidSha256));
    }
}

#[test]
fn history_longer_than_two_is_rejected() {
    let mut manifest = sample_manifest();
    manifest.history = vec![
        ReleaseHistoryEntry {
            version: "0.1.2".to_owned(),
            released_at: "2026-11-04T10:00:00Z".to_owned(),
            notes: vec![],
        },
        ReleaseHistoryEntry {
            version: "0.1.1".to_owned(),
            released_at: "2026-11-03T10:00:00Z".to_owned(),
            notes: vec![],
        },
        ReleaseHistoryEntry {
            version: "0.1.0".to_owned(),
            released_at: "2026-11-02T10:00:00Z".to_owned(),
            notes: vec![],
        },
    ];
    let (bytes, sig, public_key) = signed(&manifest);
    let error = verify_manifest(&bytes, &sig, &public_key).expect_err("history 超限");
    assert!(matches!(error, ReleaseError::TooMuchHistory));
}

#[test]
fn prerelease_manifest_versions_are_rejected() {
    for bad in ["0.2.0-dev", "0.2.0+abc", "0.2.0-dev+abc", "v0.2.0"] {
        let mut manifest = sample_manifest();
        manifest.version = bad.to_owned();
        let (bytes, sig, public_key) = signed(&manifest);
        let error = verify_manifest(&bytes, &sig, &public_key).expect_err("非稳定版本");
        assert!(matches!(error, ReleaseError::InvalidVersion(_)), "{bad}");
    }

    let mut manifest = sample_manifest();
    manifest.min_supported = "0.1.0-dev".to_owned();
    let (bytes, sig, public_key) = signed(&manifest);
    let error = verify_manifest(&bytes, &sig, &public_key).expect_err("非稳定最低版本");
    assert!(matches!(error, ReleaseError::InvalidVersion(_)));
}

#[test]
fn version_comparison_handles_development_builds() {
    assert_eq!(is_newer("0.2.0", "0.1.0"), VersionVerdict::Newer);
    assert_eq!(is_newer("0.2.0", "0.2.0"), VersionVerdict::NotNewer);
    assert_eq!(is_newer("0.1.9", "0.2.0"), VersionVerdict::NotNewer);
    assert_eq!(is_newer("1.0.0", "0.99.99"), VersionVerdict::Newer);

    // 开发构建：git describe 的短 SHA、带预发布/构建元数据的版本。
    for current in [
        "c22934c",
        "0.1.0-dev+abc",
        "0.1.0-dev",
        "0.1.0+abc",
        "v0.1.0",
    ] {
        assert_eq!(
            is_newer("0.2.0", current),
            VersionVerdict::DevelopmentBuild,
            "{current}"
        );
    }

    // 防御性：清单版本不可解析时不提示升级。
    assert_eq!(is_newer("not-a-version", "0.1.0"), VersionVerdict::NotNewer);
}

#[test]
fn below_min_supported_compares_versions() {
    let manifest = sample_manifest();
    assert_eq!(
        below_min_supported("0.0.9", &manifest),
        MinSupportVerdict::BelowMinimum
    );
    assert_eq!(
        below_min_supported("0.2.0", &manifest),
        MinSupportVerdict::Supported
    );
    assert_eq!(
        below_min_supported("0.3.0", &manifest),
        MinSupportVerdict::Supported
    );
    for current in ["c22934c", "0.1.0-dev+abc"] {
        assert_eq!(
            below_min_supported(current, &manifest),
            MinSupportVerdict::DevelopmentBuild,
            "{current}"
        );
    }

    let mut malformed = manifest;
    malformed.min_supported = "not-a-version".to_owned();
    assert_eq!(
        below_min_supported("0.1.0", &malformed),
        MinSupportVerdict::Supported
    );
}

#[test]
fn public_key_constant_matches_deploy_file() {
    let published = include_str!("../../../../deploy/release-signing.pub");
    let decoded = BASE64
        .decode(published.trim())
        .expect("deploy/release-signing.pub 是标准 Base64");
    let published: [u8; 32] = decoded.try_into().expect("公钥必须是 32 字节");
    assert_eq!(RELEASE_PUBLIC_KEY, published);
}
