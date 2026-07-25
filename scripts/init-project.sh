#!/bin/bash
# 从本仓库派生一个新项目。
#
# 与已删除的 tibba-scaffold 的区别：不再维护一份模板副本，而是直接以主应用为模板
# 复制整个仓库后改名。因此不存在依赖版本 pin 漂移——产物永远就是 CI 验证过的这份代码。
# 派生出的项目保留 workspace 与全部 tibba-* path 依赖，可直接修改框架本身。
#
# 若你想要的是「只依赖已发布 crate 的独立项目」，见 docs/scaffold.md。
set -euo pipefail

SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

usage() {
    cat <<'EOF'
用法:
  ./scripts/init-project.sh <项目名> <目标目录> [选项]

参数:
  <项目名>     新项目 / 二进制名，须为合法 crate 名（小写字母、数字、-）
  <目标目录>   父目录；项目会创建在 <目标目录>/<项目名>，该路径必须不存在

选项:
  --minimal    把应用的 default features 设为空（去掉 demo-docker / demo-detector /
               demo-tenant / demo-token 四个样板业务）
  --dry-run    只打印将要执行的替换，不写任何文件
  -h, --help   显示本帮助

示例:
  ./scripts/init-project.sh my-app ~/github
  ./scripts/init-project.sh my-app ~/github --minimal

改名范围（仅作用于副本）:
  Cargo.toml        name / default-run / keywords
  src/state.rs      AppState 的服务名
  src/**/*.rs       tracing target 前缀 "tibba:xxx" → "<项目名>:xxx"
  src/config.rs     配置环境变量前缀 TIBBA_WEB__ → <项目名大写>_WEB__
  src/main.rs       TIBBA_THREADS / TIBBA_PANIC_WECOM_KEY 前缀
  Dockerfile        镜像内路径、二进制名、运行用户
  entrypoint.sh     默认二进制名
  README.md         环境变量前缀示例

不改动 tibba-* crate 名——它们是依赖，继续以 path 形式指向副本内的同名目录。
EOF
}

NAME=""
DEST=""
MINIMAL=0
DRY_RUN=0

while [ $# -gt 0 ]; do
    case "$1" in
        -h | --help)
            usage
            exit 0
            ;;
        --minimal) MINIMAL=1 ;;
        --dry-run) DRY_RUN=1 ;;
        -*)
            echo "error: 未知选项 $1" >&2
            exit 1
            ;;
        *)
            if [ -z "$NAME" ]; then
                NAME="$1"
            elif [ -z "$DEST" ]; then
                DEST="$1"
            else
                echo "error: 多余参数 $1" >&2
                exit 1
            fi
            ;;
    esac
    shift
done

if [ -z "$NAME" ] || [ -z "$DEST" ]; then
    usage
    exit 1
fi

# crate 名限制：cargo 允许字母数字与 - _，这里收紧为小写开头，避免生成非法包名
if ! printf '%s' "$NAME" | grep -qE '^[a-z][a-z0-9-]*$'; then
    # ${NAME} 必须加花括号：macOS bash 3.2 在非 UTF-8 locale 下会把紧跟的全角
    # 「）」吞进变量名，导致 set -u 报 unbound variable
    echo "error: 项目名须为小写字母开头，仅含小写字母、数字与 -（当前: ${NAME}）" >&2
    exit 1
fi
if [ "$NAME" = "tibba" ]; then
    echo "error: 项目名不能是 tibba（与被替换的原名相同，改名会无从判断）" >&2
    exit 1
fi

# 目标目录必须不存在：本脚本会写入大量文件，绝不覆盖既有内容
TARGET="${DEST%/}/$NAME"
if [ -e "$TARGET" ]; then
    echo "error: 目标已存在，请换个路径或先移除: $TARGET" >&2
    exit 1
fi

# 环境变量前缀用大写并把 - 换成 _（TIBBA_WEB__ 的同构形式）
UPPER="$(printf '%s' "$NAME" | tr '[:lower:]-' '[:upper:]_')"

echo "源仓库:   $SRC_DIR"
echo "目标:     $TARGET"
echo "项目名:   $NAME"
echo "env 前缀: ${UPPER}_WEB__（原 TIBBA_WEB__）"
[ "$MINIMAL" -eq 1 ] && echo "features: default = []（去掉全部 demo-*）"
echo

if [ "$DRY_RUN" -eq 1 ]; then
    echo "[dry-run] 将执行的替换："
    echo "  Cargo.toml:     name/default-run/keywords \"tibba\" → \"$NAME\""
    echo "  src/state.rs:   .with_name(\"tibba\") → .with_name(\"$NAME\")"
    echo "  src/**/*.rs:    \"tibba: → \"$NAME:  （$(grep -rhoE '"tibba:[a-z:_]+"' "$SRC_DIR/src" | wc -l | tr -d ' ') 处 tracing target）"
    echo "  src/*.rs:       TIBBA_WEB / TIBBA_THREADS / TIBBA_PANIC_WECOM_KEY → ${UPPER}_*"
    echo "  Dockerfile:     \\btibba\\b → $NAME"
    echo "  entrypoint.sh:  \\btibba\\b → $NAME"
    [ "$MINIMAL" -eq 1 ] && echo "  Cargo.toml:     default = [\"full\"] → default = []"
    echo
    echo "[dry-run] 未写入任何文件。"
    exit 0
fi

mkdir -p "$TARGET"

# 复制仓库：排除构建产物与 VCS 元数据。用 tar 管道以保留权限（entrypoint.sh 的 +x）
echo "复制仓库（排除 target/ .git/ node_modules/ admin/dist/）..."
tar -C "$SRC_DIR" \
    --exclude='./target' \
    --exclude='./.git' \
    --exclude='./admin/node_modules' \
    --exclude='./admin/dist' \
    --exclude='./.DS_Store' \
    -cf - . | tar -C "$TARGET" -xf -

# admin/dist 是 vite 构建产物，不复制内容；但该目录**必须存在**——
# src/admin_web.rs 的 #[derive(RustEmbed)] #[folder = "admin/dist/"] 在编译期
# 要求 folder 存在，缺了会让整个应用编译失败（rust-embed 直接报错，不是警告）。
# 本仓库把 admin/dist/README.md 纳入 git 正是为此，这里照做。
mkdir -p "$TARGET/admin/dist"
if [ -f "$SRC_DIR/admin/dist/README.md" ]; then
    cp "$SRC_DIR/admin/dist/README.md" "$TARGET/admin/dist/README.md"
fi

cd "$TARGET"

echo "改名..."

# 应用自身的包标识
perl -i -pe 's/^name = "tibba"$/name = "'"$NAME"'"/; s/^default-run = "tibba"$/default-run = "'"$NAME"'"/; s/^keywords = \["tibba"\]$/keywords = ["'"$NAME"'"]/' Cargo.toml

# AppState 服务名
perl -i -pe 's/\.with_name\("tibba"\)/.with_name("'"$NAME"'")/' src/state.rs

# tracing target 前缀。只匹配 "tibba: 这一形式，故不会碰到 tibba-cache 等 crate 名
find src -name '*.rs' -type f -exec perl -i -pe 's/"tibba:/"'"$NAME"':/g' {} +

# 配置环境变量前缀。TIBBA_WEB 后面紧跟 __ 时**不能**在尾部加 \b——`_` 是单词字符，
# \b 不成立，会漏掉 TIBBA_WEB__BASIC__SECRET 这类。故只在头部加边界。
# 范围是整个副本：该前缀还出现在若干库 crate 的文档注释/错误消息与 configs 注释里
# （均为字符串与注释，无功能代码），一并改掉才不会给出错误的运维提示。
find . \( -name '*.rs' -o -name '*.md' -o -name '*.toml' \) -type f \
    -exec perl -i -pe 's/\bTIBBA_WEB/'"$UPPER"'_WEB/g' {} +

# 运行时环境变量（仅主应用与 README 用到）
find src -name '*.rs' -type f -exec perl -i -pe '
    s/\bTIBBA_THREADS\b/'"$UPPER"'_THREADS/g;
    s/\bTIBBA_PANIC_WECOM_KEY\b/'"$UPPER"'_PANIC_WECOM_KEY/g;
' {} +
perl -i -pe '
    s/\bTIBBA_THREADS\b/'"$UPPER"'_THREADS/g;
    s/\bTIBBA_PANIC_WECOM_KEY\b/'"$UPPER"'_PANIC_WECOM_KEY/g;
' README.md

# 镜像内路径 / 二进制名 / 运行用户。\b 词边界确保不改 tibba-* crate 名
perl -i -pe 's/\btibba\b/'"$NAME"'/g' Dockerfile entrypoint.sh

if [ "$MINIMAL" -eq 1 ]; then
    perl -i -pe 's/^default = \["full"\]$/default = []/' Cargo.toml
fi

# 项目自己的 README 标题，避免新项目顶着 tibba 的名字
perl -i -pe 's/^# tibba$/# '"$NAME"'/ if $. == 1' README.md

echo
echo "项目 '$NAME' 已创建于 '$TARGET'"
cat <<EOF

接下来:
  cd $TARGET
  git init && git add -A && git commit -m "init from tibba"

  # 数据库（PostgreSQL）
  psql -d <db> -f sql/pg/init.sql

  # 管理端 SPA
  cd admin && npm install && npm run build && cd ..

  # 配置：至少覆盖密钥与连接串（详见 README.md）
  export ${UPPER}_WEB__BASIC__SECRET='<不少于 32 字符的随机串>'
  export ${UPPER}_WEB__DATABASE__URI='postgres://...'
  export ${UPPER}_WEB__REDIS__URI='redis://...'

  cargo run

说明:
  - 副本保留了 workspace 与全部 tibba-* path 依赖，可直接改框架源码。
  - 若只想要依赖已发布 crate 的独立项目，见 docs/scaffold.md。
EOF
