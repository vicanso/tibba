#!/bin/bash
# 按「核心 / 扩展」分组发布 tibba-* 到 crates.io。
# 分层说明见 docs/crates.md；各 crate README 中有 **分层** 标记。
set -euo pipefail

# ── 核心（Core）：脚手架底座，按依赖 DAG 分批 ─────────────────────────────
# Batch C1 — 无内部依赖
CORE_C1=(
    tibba-error
    tibba-state
    tibba-performance
    tibba-validator
)
# Batch C2 — 主要依赖 error
CORE_C2=(
    tibba-util
    tibba-config
    tibba-crypto
    tibba-hook
    tibba-scheduler
)
# Batch C3 — config / util / crypto
CORE_C3=(
    tibba-model
    tibba-cache
    tibba-opendal
    tibba-request
    tibba-sql
)
# Batch C4 — model
CORE_C4=(
    tibba-model-builtin
)
# Batch C5 — cache / util / state
CORE_C5=(
    tibba-session
    tibba-email
    tibba-oauth
    tibba-jwt
    tibba-totp
    tibba-i18n
)
# Batch C6 — session / cache
CORE_C6=(
    tibba-middleware
    tibba-rbac
)
# Batch C7 — 路由层
CORE_C7=(
    tibba-router-common
    tibba-router-file
    tibba-router-model
    tibba-router-user
)

# ── 扩展（Extension）：可选能力，依赖核心 ────────────────────────────────
# Batch E1 — 仅/主要依赖 error 或 cache
EXT_E1=(
    tibba-job
    tibba-llm
    tibba-feature
)
# Batch E2 — model / job / request 等
EXT_E2=(
    tibba-model-token
    tibba-notify
    tibba-webhook
    tibba-tenant
)

# 索引刷新等待（秒）；可用环境变量覆盖
WAIT_SECS="${TIBBA_PUBLISH_WAIT:-30}"

usage() {
    cat <<'EOF'
用法:
  ./scripts/publish.sh              # 发布全部：core → ext
  ./scripts/publish.sh all          # 同上
  ./scripts/publish.sh core         # 仅核心
  ./scripts/publish.sh ext          # 仅扩展（请先保证 core 已发布）
  ./scripts/publish.sh <crate>      # 发布单个 crate，如 tibba-error

环境变量:
  TIBBA_PUBLISH_WAIT  每批之间等待 crates.io 索引的秒数（默认 30）
EOF
}

publish_one() {
    local pkg="$1"
    echo "==> publishing $pkg ..."
    cargo publish --registry crates-io -p "$pkg"
}

publish_batch() {
    local label="$1"
    shift
    local pkgs=("$@")
    if [ "${#pkgs[@]}" -eq 0 ]; then
        return 0
    fi
    echo ""
    echo "======== batch: $label ========"
    for pkg in "${pkgs[@]}"; do
        publish_one "$pkg"
    done
    echo "batch $label done, waiting ${WAIT_SECS}s for crates.io index..."
    sleep "$WAIT_SECS"
}

publish_core() {
    echo "######## CORE ########"
    publish_batch "core/C1 (leaf)" "${CORE_C1[@]}"
    publish_batch "core/C2 (error)" "${CORE_C2[@]}"
    publish_batch "core/C3 (infra)" "${CORE_C3[@]}"
    publish_batch "core/C4 (model-builtin)" "${CORE_C4[@]}"
    publish_batch "core/C5 (session/auth)" "${CORE_C5[@]}"
    publish_batch "core/C6 (middleware)" "${CORE_C6[@]}"
    # 最后一批仍等待，避免紧接着发 ext 时索引未更新
    publish_batch "core/C7 (routers)" "${CORE_C7[@]}"
    echo "######## CORE done ########"
}

publish_ext() {
    echo "######## EXTENSION ########"
    publish_batch "ext/E1" "${EXT_E1[@]}"
    publish_batch "ext/E2" "${EXT_E2[@]}"
    echo "######## EXTENSION done ########"
}

list_known() {
    printf '%s\n' \
        "${CORE_C1[@]}" "${CORE_C2[@]}" "${CORE_C3[@]}" "${CORE_C4[@]}" \
        "${CORE_C5[@]}" "${CORE_C6[@]}" "${CORE_C7[@]}" \
        "${EXT_E1[@]}" "${EXT_E2[@]}"
}

cmd="${1:-all}"

case "$cmd" in
    -h | --help | help)
        usage
        exit 0
        ;;
    all | "")
        publish_core
        publish_ext
        echo "all core + extension crates published successfully"
        ;;
    core)
        publish_core
        ;;
    ext | extension)
        publish_ext
        ;;
    tibba-*)
        # 单包：必须在清单内（防止误发 scaffold 等）
        if ! list_known | grep -qx "$cmd"; then
            echo "error: unknown or non-publishable crate: $cmd" >&2
            echo "hint: scaffold 等 tool 不发布；清单见 docs/crates.md" >&2
            exit 1
        fi
        publish_one "$cmd"
        ;;
    *)
        echo "error: unknown command: $cmd" >&2
        usage
        exit 1
        ;;
esac
