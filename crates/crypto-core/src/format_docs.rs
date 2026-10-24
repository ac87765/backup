//! 文档一致性测试（tasks 2.7）。
//!
//! `docs/crypto-format.md` 是给人看的权威说明；代码里的域分隔常量是机器唯一事实来源。
//! 二者一旦漂移，跨实现就可能悄悄不兼容，因此用测试强制文档列出每个常量。

use std::fs;
use std::path::Path;

use crate::ALL_DOMAIN_STRINGS;

#[test]
fn crypto_format_doc_lists_every_domain_string() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/crypto-format.md");
    let doc = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", path.display()));

    assert!(doc.contains("0x01"), "文档必须说明当前密文格式版本 0x01");

    for constant in ALL_DOMAIN_STRINGS {
        let text = std::str::from_utf8(constant)
            .unwrap_or_else(|error| panic!("域常量不是合法 UTF-8：{error}"));
        assert!(
            doc.contains(text),
            "docs/crypto-format.md 缺少域分隔字符串 `{text}`；新增/修改常量时必须同步文档"
        );
    }
}
