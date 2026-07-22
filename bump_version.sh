#!/bin/bash
# 统一升版本：只改根 Cargo.toml 的 [workspace.package] version，
# 并同步各成员 path 依赖里的 version = "..."（crates.io 发布需要）。
set -euo pipefail

NEW_VERSION="${1:?用法: $0 <新版本号>  例如: $0 0.2.3}"

ROOT="$(cd "$(dirname "$0")" && pwd)"
ROOT_TOML="$ROOT/Cargo.toml"

if ! grep -q '^\[workspace\.package\]' "$ROOT_TOML"; then
    echo "error: $ROOT_TOML 缺少 [workspace.package]，无法统一升版本" >&2
    exit 1
fi

# 仅替换 [workspace.package] 段内的 version = "..."
# 用 awk：进入 workspace.package 后改 version，直到下一个 [section]
awk -v ver="$NEW_VERSION" '
  BEGIN { in_wp = 0 }
  /^\[workspace\.package\]/ { in_wp = 1; print; next }
  /^\[/ { in_wp = 0 }
  in_wp && /^version = "/ {
    print "version = \"" ver "\""
    next
  }
  { print }
' "$ROOT_TOML" > "$ROOT_TOML.tmp"
mv "$ROOT_TOML.tmp" "$ROOT_TOML"
echo "workspace.package.version -> ${NEW_VERSION}"

# path 依赖上的 version 必须与被依赖 crate 的 package version 一致（publish 用）
for toml in "$ROOT"/Cargo.toml "$ROOT"/tibba-*/Cargo.toml; do
    # 只改 tibba-* path 依赖行里的 version，不动第三方 crate
    if grep -q 'tibba-[a-z0-9-]* = {.*version = "' "$toml" 2>/dev/null; then
        sed -i '' -E "s/(tibba-[a-z0-9-]+ = \{[^}]*version = \")[^\"]+\"/\1${NEW_VERSION}\"/" "$toml"
        echo "updated path dep versions in $toml"
    fi
done

# 变量名后紧跟中文全角字符时必须加花括号：
# macOS 自带 bash 3.2 在非 UTF-8 locale 下会把高位字节当作合法标识符字符，
# `$NEW_VERSION（` 会被解析成变量 `NEW_VERSION（via`，触发 set -u 的 unbound variable
echo "所有模块版本已统一为 ${NEW_VERSION}（via workspace.package）"
