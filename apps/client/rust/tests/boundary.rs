//! 桥接边界测试（design D2，tasks 4.2）。
//!
//! 自动解析 `src/api/**/*.rs` 中所有 `pub struct` / `pub enum`，断言：
//! 1. 字段类型不含字节数组（`Vec<u8>`、`[u8; N]`、`ZeroCopyBuffer`、`bytes::Bytes` 等）；
//! 2. 字段名不含 key / secret / token / password / auth（输入参数里的 password 是
//!    函数参数，不在 DTO 字段范围内）。
//!
//! 新增 DTO 时无需修改本测试：只要加在 `src/api` 下就会被扫描。测试末尾的反例
//! 自检保证检查器本身有效。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
struct Violation {
    file: String,
    item: String,
    detail: String,
}

fn api_sources() -> Vec<PathBuf> {
    fn visit(directory: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                out.push(path);
            }
        }
    }

    let mut sources = Vec::new();
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/api"),
        &mut sources,
    );
    sources.sort();
    sources
}

/// 解析出 (类型名, 声明体) 列表。
fn declarations(source: &str) -> Vec<(String, String)> {
    let bytes = source.as_bytes();
    let mut items = Vec::new();
    let mut cursor = 0;
    loop {
        let struct_at = source[cursor..].find("pub struct");
        let enum_at = source[cursor..].find("pub enum");
        let offset = match (struct_at, enum_at) {
            (Some(left), Some(right)) => left.min(right),
            (Some(left), None) => left,
            (None, Some(right)) => right,
            (None, None) => break,
        };
        let start = cursor + offset;
        let keyword_len = if source[start..].starts_with("pub struct") {
            "pub struct".len()
        } else {
            "pub enum".len()
        };
        let rest = &source[start + keyword_len..];
        let name_start = rest
            .find(|character: char| !character.is_whitespace())
            .unwrap_or(rest.len());
        let name_end = rest[name_start..]
            .find(|character: char| character.is_whitespace() || matches!(character, '{' | '('))
            .map(|offset| name_start + offset)
            .unwrap_or(rest.len());
        let name = rest[name_start..name_end].trim().to_owned();
        let after_name = start + keyword_len + name_end;
        let body_start = source[after_name..]
            .find(['{', '(', ';'])
            .map(|index| after_name + index);
        let Some(body_start) = body_start else {
            break;
        };
        if bytes[body_start] == b';' {
            cursor = body_start + 1;
            continue;
        }
        let open = bytes[body_start] as char;
        let close = if open == '{' { '}' } else { ')' };
        let mut depth = 0_i32;
        let mut body_end = source.len();
        for (index, character) in source[body_start..].char_indices() {
            if character == open {
                depth += 1;
            } else if character == close {
                depth -= 1;
                if depth == 0 {
                    body_end = body_start + index;
                    break;
                }
            }
        }
        items.push((name, source[body_start + 1..body_end].to_owned()));
        cursor = body_end.saturating_add(1);
    }
    items
}

fn check_body(item: &str, body: &str, file: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    // 把 struct 字段、enum 变体字段与元组字段拆成独立片段；随后按 `字段名: 类型`
    // 解析。泛型参数里的逗号（如 HashMap<K, V>）也会被拆开，但不会产生假字段名。
    for segment in body.split([',', '{', '}', '(', ')', '\n']) {
        let segment = segment.trim().trim_start_matches("pub ").trim();
        if segment.is_empty() || segment.starts_with("//") || segment.starts_with('#') {
            continue;
        }
        let (field_name, type_part) = match segment.split_once(':') {
            Some((name, ty)) => (Some(name.trim()), ty.trim()),
            None => (None, segment),
        };
        if let Some(field_name) = field_name
            && is_identifier(field_name)
            && is_sensitive_name(field_name)
        {
            violations.push(Violation {
                file: file.to_owned(),
                item: item.to_owned(),
                detail: format!("字段 `{field_name}` 命中敏感名称检查"),
            });
        }
        if let Some(reason) = byte_type_reason(type_part) {
            let label = field_name
                .filter(|name| is_identifier(name))
                .unwrap_or(item);
            violations.push(Violation {
                file: file.to_owned(),
                item: item.to_owned(),
                detail: format!("字段 `{label}` {reason}"),
            });
        }
    }
    violations
}

fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn is_sensitive_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["key", "secret", "token", "password", "auth"]
        .iter()
        .any(|needle| name.contains(needle))
}

fn byte_type_reason(type_part: &str) -> Option<String> {
    let normalized: String = type_part
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    if normalized.contains("vec<u8") {
        return Some("使用了 Vec<u8> 字节数组".to_owned());
    }
    if normalized.contains("[u8;") || normalized.contains("[u8]") {
        return Some("使用了 [u8; N] / [u8] 字节数组".to_owned());
    }
    if normalized.contains("zerocopybuffer") {
        return Some("使用了 ZeroCopyBuffer".to_owned());
    }
    if normalized.contains("bytes::bytes") {
        return Some("使用了 bytes::Bytes".to_owned());
    }
    if normalized.contains("&[u8]") || normalized.contains("cow<") && normalized.contains("[u8]") {
        return Some("使用了字节切片".to_owned());
    }
    None
}

fn scan_source(file: &str, source: &str) -> Vec<Violation> {
    declarations(source)
        .into_iter()
        .flat_map(|(item, body)| check_body(&item, &body, file))
        .collect()
}

#[test]
fn all_bridge_dtos_are_free_of_bytes_and_secret_fields() {
    let sources = api_sources();
    assert!(
        sources.len() >= 7,
        "应扫描到 src/api 下的全部模块，实际 {} 个文件",
        sources.len()
    );

    let mut all = Vec::new();
    let mut item_names = Vec::new();
    for path in &sources {
        let source = fs::read_to_string(path).unwrap();
        for (name, _) in declarations(&source) {
            item_names.push(name);
        }
        all.extend(scan_source(
            &path.file_name().unwrap().to_string_lossy(),
            &source,
        ));
    }

    assert!(
        item_names.iter().any(|name| name == "AccountDto"),
        "边界检查器没有解析到 AccountDto，说明解析逻辑失效"
    );
    assert!(
        item_names.len() >= 30,
        "DTO 数量异常偏少（{}），新增 DTO 可能没有被扫描",
        item_names.len()
    );
    assert!(all.is_empty(), "桥接 DTO 违反 design D2 密钥边界：{all:#?}");
}

#[test]
fn bridge_functions_do_not_return_byte_arrays() {
    let sources = api_sources();
    let mut violations = Vec::new();
    for path in &sources {
        let source = fs::read_to_string(path).unwrap();
        let mut rest = source.as_str();
        while let Some(position) = rest.find("->") {
            let after = &rest[position + 2..];
            let end = after.find('{').unwrap_or(after.len());
            if let Some(reason) = byte_type_reason(&after[..end]) {
                violations.push(format!("{}: 返回类型{reason}", path.display()));
            }
            rest = &after[end..];
        }
    }
    assert!(
        violations.is_empty(),
        "桥接函数返回值违反 design D2：{violations:#?}"
    );
}

#[test]
fn detector_rejects_byte_arrays_and_secret_field_names() {
    let sample = r#"
        pub struct LeakyDto {
            pub id: String,
            pub data: Vec<u8>,
            pub auth_token: String,
        }
        pub enum LeakyEnum {
            Blob([u8; 32]),
            Plain(String),
        }
    "#;
    let violations = scan_source("sample.rs", sample);
    assert_eq!(violations.len(), 3, "反例应报告 3 个问题：{violations:#?}");
    assert!(
        violations
            .iter()
            .any(|violation| violation.detail.contains("Vec<u8>"))
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.detail.contains("auth_token"))
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.detail.contains("[u8; N]"))
    );
}
