//! `release-tool` 命令行入口：把错误转成中文消息与非零退出码。

use std::ffi::OsString;

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if let Err(error) = release_tool::run(args) {
        eprintln!("错误：{error}");
        std::process::exit(error.exit_code());
    }
}
