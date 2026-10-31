#!/usr/bin/env bash
#
# 加密备份 macOS 安装包打包脚本。
#
# 只在 macOS 上运行（CI 的 macos 运行器，或排查 CI 打包问题时在 Mac 本地运行）：
#   1. flutter build macos --release（注入 APP_VERSION / GIT_COMMIT）
#   2. ad-hoc 签名：codesign --force --deep --sign -，并 --verify --deep --strict
#   3. hdiutil 生成只读压缩 dmg，内含“加密备份.app”与 /Applications 快捷方式
#
# 用法：
#   bash apps/client/tool/package_macos.sh
#   APP_VERSION=1.2.3 GIT_COMMIT=0123abc bash apps/client/tool/package_macos.sh
#   SERVER_URL=https://example.com bash apps/client/tool/package_macos.sh
#
# 产物：apps/client/build/installer/加密备份-<版本>-macos.dmg
#
# 架构说明：Flutter macOS release 默认只构建运行构建机当前架构的产物
# （macos-latest 为 Apple Silicon，产物是 arm64；Intel Mac 上是 x86_64）。
# 要得到 universal，需要额外用 xcodebuild ARCHS="arm64 x86_64" 构建；
# 本脚本不改变 Flutter 默认行为，只把 lipo -archs 的结果打印到日志，供验收核对。
#
# 自用分发，不做 Apple 公证（design D10）。应用未启用 App Sandbox。
# 本脚本只写 apps/client/build/ 与 mktemp 临时目录；不修改系统设置。

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLIENT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$CLIENT_DIR/../.." && pwd)"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: 本脚本只能在 macOS 上运行（当前：$(uname -s)）。" >&2
  exit 1
fi

if [[ -z "${APP_VERSION:-}" ]]; then
  APP_VERSION="$(git -C "$REPO_ROOT" describe --tags --always --dirty)"
fi
if [[ -z "${GIT_COMMIT:-}" ]]; then
  GIT_COMMIT="$(git -C "$REPO_ROOT" rev-parse --short HEAD)"
fi

APP_NAME="加密备份"
APP_PATH="$CLIENT_DIR/build/macos/Build/Products/Release/$APP_NAME.app"
OUT_DIR="$CLIENT_DIR/build/installer"
# dmg 文件名不允许 / 与 :；git 标签允许它们。
SAFE_VERSION="$(printf '%s' "$APP_VERSION" | tr '/:' '--' | tr '[:space:]' '-')"
DMG_PATH="$OUT_DIR/$APP_NAME-$SAFE_VERSION-macos.dmg"

DART_DEFINES=(
  "--dart-define=APP_VERSION=$APP_VERSION"
  "--dart-define=GIT_COMMIT=$GIT_COMMIT"
)
if [[ -n "${SERVER_URL:-}" ]]; then
  DART_DEFINES+=("--dart-define=SERVER_URL=$SERVER_URL")
fi

echo "构建 macOS Release：APP_VERSION=$APP_VERSION GIT_COMMIT=$GIT_COMMIT"
(
  cd "$CLIENT_DIR"
  flutter build macos --release "${DART_DEFINES[@]}"
)

if [[ ! -d "$APP_PATH" ]]; then
  echo "error: 找不到构建产物：$APP_PATH" >&2
  exit 1
fi

echo "ad-hoc 签名：$APP_PATH"
# 清掉构建产物上的扩展属性，避免 codesign 报 resource fork / Finder info 错误。
xattr -cr "$APP_PATH"
codesign --force --deep --sign - "$APP_PATH"
codesign --verify --deep --strict --verbose=2 "$APP_PATH"

BINARY="$APP_PATH/Contents/MacOS/$APP_NAME"
if [[ ! -f "$BINARY" ]]; then
  echo "error: 找不到可执行文件：$BINARY" >&2
  exit 1
fi
echo "应用架构（lipo -archs）：$(lipo -archs "$BINARY")"

STAGING_DIR="$(mktemp -d "${TMPDIR:-/tmp}/encrypted-backup-dmg.XXXXXX")"
cleanup() {
  rm -rf "$STAGING_DIR"
}
trap cleanup EXIT

# ditto 保留应用包内的符号链接、权限与代码签名；/Applications 是拖拽目标。
ditto "$APP_PATH" "$STAGING_DIR/$APP_NAME.app"
ln -s /Applications "$STAGING_DIR/Applications"

mkdir -p "$OUT_DIR"
echo "生成 dmg：$DMG_PATH"
hdiutil create \
  -volname "$APP_NAME" \
  -srcfolder "$STAGING_DIR" \
  -ov \
  -format UDZO \
  "$DMG_PATH"
hdiutil verify "$DMG_PATH"

echo "dmg：$DMG_PATH"
