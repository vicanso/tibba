#!/bin/bash
set -euo pipefail

NEW_VERSION="${1:?用法: $0 <新版本号>  例如: $0 0.2.0}"

ROOT="$(cd "$(dirname "$0")" && pwd)"

update() {
    local file="$1"
    # 替换 crate 自身版本：行首 version = "x.x.x"
    sed -i '' "s/^version = \"[^\"]*\"/version = \"$NEW_VERSION\"/" "$file"
    # 替换依赖中 tibba-* 的版本：tibba-xxx = { ..., version = "x.x.x" ... }
    sed -i '' "s/\(tibba-[a-z-]* = {[^}]*version = \"\)[^\"]*\"/\1$NEW_VERSION\"/" "$file"
    echo "updated $file"
}

update "$ROOT/Cargo.toml"
for toml in "$ROOT"/tibba-*/Cargo.toml; do
    update "$toml"
done

echo "所有模块版本已更新为 $NEW_VERSION"
