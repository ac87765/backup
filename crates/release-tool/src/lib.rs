//! `release-tool` —— 维护者电脑上的客户端发布工具。
//!
//! 子命令：
//! - `keygen`：生成 Ed25519 密钥对（私钥只写本地文件，不打印）。
//! - `manifest`：计算安装包大小与 SHA-256，生成稳定格式的 `latest.json`。
//! - `sign`：用私钥对清单原始字节签名，输出 Base64 一行。
//! - `verify`：用公钥验签并解析清单，可选复核安装包大小与 SHA-256。
//!
//! 私钥只在本进程内存中出现，读入的 Base64 文本与种子离开作用域时清零；任何
//! 输出与错误信息都不包含私钥内容。退出码：0 成功，2 参数错误，1 其他失败。

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use client_core::release::{
    MANIFEST_SCHEMA, MAX_HISTORY, RELEASE_PUBLIC_KEY, ReleaseArtifact, ReleaseHistoryEntry,
    ReleaseManifest, ReleasePlatforms, parse_stable_version, verify_manifest,
};
use ed25519_dalek::{Signer, SigningKey};
use semver::Version;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

/// 命令失败：携带给用户的中文消息与退出码。
#[derive(Debug)]
pub struct ToolError {
    message: String,
    exit_code: i32,
}

impl ToolError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 2,
        }
    }

    fn failed(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 1,
        }
    }

    /// 进程应使用的退出码（0 不会出现在错误中）。
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ToolError {}

/// 执行一条命令；`args` 不含程序名。
pub fn run<I>(args: I) -> Result<(), ToolError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Err(ToolError::usage(USAGE));
    };
    let command = command
        .to_str()
        .ok_or_else(|| ToolError::usage("命令名不是有效的 UTF-8"))?;
    let rest: Vec<OsString> = args.collect();
    match command {
        "keygen" => run_keygen(parse_keygen_args(rest)?),
        "manifest" => run_manifest(parse_manifest_args(rest)?),
        "sign" => run_sign(parse_sign_args(rest)?),
        "verify" => run_verify(parse_verify_args(rest)?),
        "--help" | "-h" | "help" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(ToolError::usage(format!("未知命令：{other}\n\n{USAGE}"))),
    }
}

const USAGE: &str = "\
用法：
  release-tool keygen --out <私钥路径> --pub <公钥路径>
  release-tool manifest --version X.Y.Z --released-at <RFC3339> --min-supported X.Y.Z \
--notes <每行一条的文本文件> --windows <exe 路径> --macos <dmg 路径> \
[--previous <上一份 latest.json>] --out <latest.json>
  release-tool sign --key <私钥路径> --in <latest.json> --out <latest.json.sig>
  release-tool verify [--pub <公钥路径>] --in <latest.json> --sig <sig> [--files <安装包目录>]

省略 --pub 时使用编译进客户端的发布公钥。";

struct KeygenArgs {
    out: PathBuf,
    public: PathBuf,
}

fn parse_keygen_args(args: Vec<OsString>) -> Result<KeygenArgs, ToolError> {
    let mut out = None;
    let mut public = None;
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        let flag = as_flag(&argument)?;
        match flag {
            "--out" => set_once(
                &mut out,
                PathBuf::from(next_value(&mut args, "--out")?),
                "--out",
            )?,
            "--pub" => set_once(
                &mut public,
                PathBuf::from(next_value(&mut args, "--pub")?),
                "--pub",
            )?,
            other => return Err(unknown_argument("keygen", other)),
        }
    }
    Ok(KeygenArgs {
        out: required(out, "--out")?,
        public: required(public, "--pub")?,
    })
}

struct ManifestArgs {
    version: String,
    released_at: String,
    min_supported: String,
    notes: PathBuf,
    windows: PathBuf,
    macos: PathBuf,
    previous: Option<PathBuf>,
    out: PathBuf,
}

fn parse_manifest_args(args: Vec<OsString>) -> Result<ManifestArgs, ToolError> {
    let mut version = None;
    let mut released_at = None;
    let mut min_supported = None;
    let mut notes = None;
    let mut windows = None;
    let mut macos = None;
    let mut previous = None;
    let mut out = None;
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        let flag = as_flag(&argument)?;
        match flag {
            "--version" => set_once(
                &mut version,
                next_string(&mut args, "--version")?,
                "--version",
            )?,
            "--released-at" => set_once(
                &mut released_at,
                next_string(&mut args, "--released-at")?,
                "--released-at",
            )?,
            "--min-supported" => set_once(
                &mut min_supported,
                next_string(&mut args, "--min-supported")?,
                "--min-supported",
            )?,
            "--notes" => set_once(
                &mut notes,
                PathBuf::from(next_value(&mut args, "--notes")?),
                "--notes",
            )?,
            "--windows" => set_once(
                &mut windows,
                PathBuf::from(next_value(&mut args, "--windows")?),
                "--windows",
            )?,
            "--macos" => set_once(
                &mut macos,
                PathBuf::from(next_value(&mut args, "--macos")?),
                "--macos",
            )?,
            "--previous" => set_once(
                &mut previous,
                PathBuf::from(next_value(&mut args, "--previous")?),
                "--previous",
            )?,
            "--out" => set_once(
                &mut out,
                PathBuf::from(next_value(&mut args, "--out")?),
                "--out",
            )?,
            other => return Err(unknown_argument("manifest", other)),
        }
    }
    Ok(ManifestArgs {
        version: required(version, "--version")?,
        released_at: required(released_at, "--released-at")?,
        min_supported: required(min_supported, "--min-supported")?,
        notes: required(notes, "--notes")?,
        windows: required(windows, "--windows")?,
        macos: required(macos, "--macos")?,
        previous,
        out: required(out, "--out")?,
    })
}

struct SignArgs {
    key: PathBuf,
    input: PathBuf,
    out: PathBuf,
}

fn parse_sign_args(args: Vec<OsString>) -> Result<SignArgs, ToolError> {
    let mut key = None;
    let mut input = None;
    let mut out = None;
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        let flag = as_flag(&argument)?;
        match flag {
            "--key" => set_once(
                &mut key,
                PathBuf::from(next_value(&mut args, "--key")?),
                "--key",
            )?,
            "--in" => set_once(
                &mut input,
                PathBuf::from(next_value(&mut args, "--in")?),
                "--in",
            )?,
            "--out" => set_once(
                &mut out,
                PathBuf::from(next_value(&mut args, "--out")?),
                "--out",
            )?,
            other => return Err(unknown_argument("sign", other)),
        }
    }
    Ok(SignArgs {
        key: required(key, "--key")?,
        input: required(input, "--in")?,
        out: required(out, "--out")?,
    })
}

struct VerifyArgs {
    public: Option<PathBuf>,
    input: PathBuf,
    sig: PathBuf,
    files: Option<PathBuf>,
}

fn parse_verify_args(args: Vec<OsString>) -> Result<VerifyArgs, ToolError> {
    let mut public = None;
    let mut input = None;
    let mut sig = None;
    let mut files = None;
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        let flag = as_flag(&argument)?;
        match flag {
            "--pub" => set_once(
                &mut public,
                PathBuf::from(next_value(&mut args, "--pub")?),
                "--pub",
            )?,
            "--in" => set_once(
                &mut input,
                PathBuf::from(next_value(&mut args, "--in")?),
                "--in",
            )?,
            "--sig" => set_once(
                &mut sig,
                PathBuf::from(next_value(&mut args, "--sig")?),
                "--sig",
            )?,
            "--files" => set_once(
                &mut files,
                PathBuf::from(next_value(&mut args, "--files")?),
                "--files",
            )?,
            other => return Err(unknown_argument("verify", other)),
        }
    }
    Ok(VerifyArgs {
        public,
        input: required(input, "--in")?,
        sig: required(sig, "--sig")?,
        files,
    })
}

fn as_flag(argument: &OsString) -> Result<&str, ToolError> {
    argument
        .to_str()
        .ok_or_else(|| ToolError::usage("参数名不是有效的 UTF-8"))
}

fn next_value(
    args: &mut impl Iterator<Item = OsString>,
    flag: &str,
) -> Result<OsString, ToolError> {
    args.next()
        .ok_or_else(|| ToolError::usage(format!("参数 {flag} 缺少值")))
}

fn next_string(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<String, ToolError> {
    next_value(args, flag)?
        .into_string()
        .map_err(|_| ToolError::usage(format!("参数 {flag} 不是有效的 UTF-8")))
}

fn set_once<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<(), ToolError> {
    if slot.is_some() {
        return Err(ToolError::usage(format!("参数 {flag} 重复")));
    }
    *slot = Some(value);
    Ok(())
}

fn required<T>(slot: Option<T>, flag: &str) -> Result<T, ToolError> {
    slot.ok_or_else(|| ToolError::usage(format!("缺少必填参数 {flag}")))
}

fn unknown_argument(command: &str, argument: &str) -> ToolError {
    ToolError::usage(format!("{command} 不认识的参数：{argument}"))
}

fn run_keygen(args: KeygenArgs) -> Result<(), ToolError> {
    // 先检查两个目标都不存在，避免生成后才发现公钥路径冲突。
    for path in [&args.out, &args.public] {
        if path.exists() {
            return Err(ToolError::failed(format!(
                "拒绝覆盖已有文件：{}",
                path.display()
            )));
        }
    }

    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed)
        .map_err(|error| ToolError::failed(format!("操作系统 CSPRNG 失败：{error}")))?;
    let signing_key = SigningKey::from_bytes(&seed);
    let public = signing_key.verifying_key().to_bytes();
    let private_base64 = Zeroizing::new(BASE64.encode(seed));
    let public_base64 = BASE64.encode(public);
    seed.zeroize();

    write_new_file(&args.out, private_base64.as_bytes())?;
    if let Err(error) = restrict_permissions(&args.out) {
        let _ = fs::remove_file(&args.out);
        return Err(error);
    }
    if let Err(error) = write_new_file(&args.public, public_base64.as_bytes()) {
        let _ = fs::remove_file(&args.out);
        return Err(error);
    }

    println!("私钥已写入：{}（内容不回显）", args.out.display());
    println!("公钥已写入：{}（{}）", args.public.display(), public_base64);
    Ok(())
}

fn run_manifest(args: ManifestArgs) -> Result<(), ToolError> {
    let version = stable_version(&args.version, "--version")?;
    if !validate_rfc3339(&args.released_at) {
        return Err(ToolError::usage(format!(
            "--released-at 不是 RFC3339 时间：{}",
            args.released_at
        )));
    }
    let min_supported = stable_version(&args.min_supported, "--min-supported")?;
    let notes = read_notes(&args.notes)?;
    let version_text = version.to_string();

    let windows = artifact(
        &args.windows,
        format!("EncryptedBackup-{version_text}-windows-x64.exe"),
    )?;
    let macos = artifact(
        &args.macos,
        format!("EncryptedBackup-{version_text}-macos.dmg"),
    )?;

    let history = if let Some(path) = &args.previous {
        let previous = read_manifest(path)?;
        let previous_version = parse_stable_version(&previous.version)
            .map_err(|error| ToolError::failed(format!("上一份清单的版本无效：{error}")))?;
        if version <= previous_version {
            return Err(ToolError::failed(format!(
                "新版本 {version} 必须高于上一份清单的版本 {}",
                previous.version
            )));
        }
        let mut history = Vec::with_capacity(MAX_HISTORY);
        history.push(ReleaseHistoryEntry {
            version: previous.version.clone(),
            released_at: previous.released_at.clone(),
            notes: previous.notes.clone(),
        });
        history.extend(previous.history.iter().cloned());
        history.truncate(MAX_HISTORY);
        history
    } else {
        Vec::new()
    };

    let manifest = ReleaseManifest {
        schema: MANIFEST_SCHEMA,
        version: version_text,
        released_at: args.released_at,
        min_supported: min_supported.to_string(),
        notes,
        history,
        platforms: ReleasePlatforms {
            windows_x64: windows,
            macos_universal: macos,
        },
    };
    manifest
        .validate()
        .map_err(|error| ToolError::failed(format!("生成的清单无效：{error}")))?;

    let mut json = serde_json::to_string_pretty(&manifest)
        .map_err(|error| ToolError::failed(format!("序列化清单失败：{error}")))?;
    json.push('\n');
    write_file(&args.out, json.as_bytes())?;
    println!(
        "清单已生成：{}（版本 {}，Windows {} 字节 / macOS {} 字节）",
        args.out.display(),
        manifest.version,
        manifest.platforms.windows_x64.size,
        manifest.platforms.macos_universal.size
    );
    Ok(())
}

fn run_sign(args: SignArgs) -> Result<(), ToolError> {
    let bytes = fs::read(&args.input).map_err(|error| {
        ToolError::failed(format!("无法读取清单 {}：{error}", args.input.display()))
    })?;
    let signing_key = read_private_key(&args.key)?;
    let signature = signing_key.sign(&bytes);
    let encoded = BASE64.encode(signature.to_bytes());
    let mut output = encoded;
    output.push('\n');
    write_file(&args.out, output.as_bytes())?;
    println!("签名已写入：{}", args.out.display());
    Ok(())
}

fn run_verify(args: VerifyArgs) -> Result<(), ToolError> {
    let bytes = fs::read(&args.input).map_err(|error| {
        ToolError::failed(format!("无法读取清单 {}：{error}", args.input.display()))
    })?;
    let signature = fs::read_to_string(&args.sig).map_err(|error| {
        ToolError::failed(format!("无法读取签名 {}：{error}", args.sig.display()))
    })?;
    let public_key = read_public_key(args.public.as_deref())?;
    let manifest = verify_manifest(&bytes, &signature, &public_key)
        .map_err(|error| ToolError::failed(format!("清单校验失败：{error}")))?;

    if let Some(directory) = &args.files {
        check_installer(directory, "windows-x64", &manifest.platforms.windows_x64)?;
        check_installer(
            directory,
            "macos-universal",
            &manifest.platforms.macos_universal,
        )?;
        println!(
            "清单有效：版本 {}；安装包大小与 SHA-256 一致",
            manifest.version
        );
    } else {
        println!("清单有效：版本 {}；签名与格式校验通过", manifest.version);
    }
    Ok(())
}

fn stable_version(value: &str, flag: &str) -> Result<Version, ToolError> {
    parse_stable_version(value).map_err(|error| ToolError::usage(format!("{flag} 无效：{error}")))
}

fn artifact(path: &Path, file: String) -> Result<ReleaseArtifact, ToolError> {
    let (size, sha256) = hash_file(path)?;
    Ok(ReleaseArtifact { file, size, sha256 })
}

fn hash_file(path: &Path) -> Result<(u64, String), ToolError> {
    let mut file = fs::File::open(path).map_err(|error| {
        ToolError::failed(format!("无法打开安装包 {}：{error}", path.display()))
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            ToolError::failed(format!("读取安装包 {} 失败：{error}", path.display()))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    Ok((size, hex::encode(hasher.finalize())))
}

fn read_notes(path: &Path) -> Result<Vec<String>, ToolError> {
    let text = fs::read_to_string(path).map_err(|error| {
        ToolError::failed(format!("无法读取更新说明 {}：{error}", path.display()))
    })?;
    // Windows 记事本可能写入 UTF-8 BOM；它不是更新内容的一部分。
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    Ok(text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect())
}

fn read_manifest(path: &Path) -> Result<ReleaseManifest, ToolError> {
    let bytes = fs::read(path).map_err(|error| {
        ToolError::failed(format!("无法读取上一份清单 {}：{error}", path.display()))
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        ToolError::failed(format!("上一份清单 {} 格式无效：{error}", path.display()))
    })
}

fn read_private_key(path: &Path) -> Result<SigningKey, ToolError> {
    let bytes = fs::read(path)
        .map_err(|error| ToolError::failed(format!("无法读取私钥 {}：{error}", path.display())))?;
    let encoded = Zeroizing::new(bytes);
    let trimmed = trim_ascii(encoded.as_slice());
    let mut decoded = BASE64
        .decode(trimmed)
        .map_err(|_| ToolError::failed("私钥不是有效的 Base64"))?;
    if decoded.len() != 32 {
        decoded.zeroize();
        return Err(ToolError::failed("私钥必须是 32 字节种子的 Base64"));
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&decoded);
    decoded.zeroize();
    let signing_key = SigningKey::from_bytes(&seed);
    seed.zeroize();
    Ok(signing_key)
}

fn read_public_key(path: Option<&Path>) -> Result<[u8; 32], ToolError> {
    match path {
        Some(path) => {
            let bytes = fs::read(path).map_err(|error| {
                ToolError::failed(format!("无法读取公钥 {}：{error}", path.display()))
            })?;
            decode_public_key(&bytes)
        }
        None => Ok(RELEASE_PUBLIC_KEY),
    }
}

fn decode_public_key(bytes: &[u8]) -> Result<[u8; 32], ToolError> {
    let text = std::str::from_utf8(trim_ascii(bytes))
        .map_err(|_| ToolError::failed("公钥文件不是有效的 UTF-8"))?;
    let decoded = BASE64
        .decode(text)
        .map_err(|_| ToolError::failed("公钥不是有效的 Base64"))?;
    decoded
        .try_into()
        .map_err(|_| ToolError::failed("公钥必须是 32 字节"))
}

fn check_installer(
    directory: &Path,
    platform: &str,
    artifact: &ReleaseArtifact,
) -> Result<(), ToolError> {
    let path = directory.join(&artifact.file);
    let (size, sha256) = hash_file(&path)?;
    if size != artifact.size {
        return Err(ToolError::failed(format!(
            "{platform} 安装包 {} 大小不一致：清单 {} 字节，实际 {} 字节",
            artifact.file, artifact.size, size
        )));
    }
    let same = sha256.len() == artifact.sha256.len()
        && bool::from(sha256.as_bytes().ct_eq(artifact.sha256.as_bytes()));
    if !same {
        return Err(ToolError::failed(format!(
            "{platform} 安装包 {} 的 SHA-256 与清单不一致",
            artifact.file
        )));
    }
    Ok(())
}

fn write_file(path: &Path, content: &[u8]) -> Result<(), ToolError> {
    fs::write(path, content)
        .map_err(|error| ToolError::failed(format!("无法写入 {}：{error}", path.display())))
}

fn write_new_file(path: &Path, content: &[u8]) -> Result<(), ToolError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| ToolError::failed(format!("无法创建 {}：{error}", path.display())))?;
    file.write_all(content)
        .map_err(|error| ToolError::failed(format!("写入 {} 失败：{error}", path.display())))?;
    file.write_all(b"\n")
        .map_err(|error| ToolError::failed(format!("写入 {} 失败：{error}", path.display())))?;
    file.sync_all()
        .map_err(|error| ToolError::failed(format!("同步 {} 失败：{error}", path.display())))
}

fn restrict_permissions(path: &Path) -> Result<(), ToolError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
            ToolError::failed(format!("设置私钥权限失败 {}：{error}", path.display()))
        })?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |index| index + 1);
    &bytes[start..end]
}

/// 粗略但严格的 RFC3339 校验：日期、时间、可选小数秒与时区偏移。
fn validate_rfc3339(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || !matches!(bytes.get(10).copied(), Some(b'T' | b't'))
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return false;
    }
    let Some(year) = parse_digits(&bytes[0..4]) else {
        return false;
    };
    let Some(month) = parse_digits(&bytes[5..7]) else {
        return false;
    };
    let Some(day) = parse_digits(&bytes[8..10]) else {
        return false;
    };
    let Some(hour) = parse_digits(&bytes[11..13]) else {
        return false;
    };
    let Some(minute) = parse_digits(&bytes[14..16]) else {
        return false;
    };
    let Some(second) = parse_digits(&bytes[17..19]) else {
        return false;
    };
    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return false;
    }

    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == start || index - start > 9 {
            return false;
        }
    }
    match bytes.get(index).copied() {
        Some(b'Z' | b'z') => {
            index += 1;
        }
        Some(b'+' | b'-') => {
            let Some(offset_hour) = bytes.get(index + 1..index + 3).and_then(parse_digits) else {
                return false;
            };
            let Some(offset_minute) = bytes.get(index + 4..index + 6).and_then(parse_digits) else {
                return false;
            };
            if bytes.get(index + 3) != Some(&b':') || offset_hour > 23 || offset_minute > 59 {
                return false;
            }
            index += 6;
        }
        _ => return false,
    }
    index == bytes.len()
}

fn parse_digits(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut value = 0u32;
    for byte in bytes {
        value = value
            .checked_mul(10)?
            .checked_add(u32::from(*byte - b'0'))?;
    }
    Some(value)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}
