#!/bin/bash
# 按「核心 / 标准 / 扩展」分组发布 tibba-* 到 crates.io。
# 分层说明见 docs/crates.md；各 crate README 中有 **分层** 标记。
set -euo pipefail

# ── 核心（Core）：最小脚手架底座，按依赖 DAG 分批 ────────────────────────
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
# Batch C3 — config / util（缓存与出站 HTTP 客户端，几乎所有请求都依赖）
CORE_C3=(
    tibba-cache
    tibba-request
)

# ── 标准（Standard）：标准 REST 构件，比 core 低、比 ext 高，依赖核心 ─────
# Batch S1 — config / util / crypto
STANDARD_S1=(
    tibba-model
    tibba-opendal
    tibba-sql
)
# Batch S2 — model
STANDARD_S2=(
    tibba-model-builtin
)
# Batch S3 — cache / util / state
STANDARD_S3=(
    tibba-session
    tibba-email
    tibba-oauth
    tibba-jwt
    tibba-totp
    tibba-i18n
)
# Batch S4 — session / cache
STANDARD_S4=(
    tibba-middleware
    tibba-rbac
)
# Batch S5 — 路由层
STANDARD_S5=(
    tibba-router-common
    tibba-router-file
    tibba-router-model
    tibba-router-user
)

# ── 扩展（Extension）：可选能力，依赖核心与标准 ──────────────────────────
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
  ./scripts/publish.sh              # 发布全部：core → standard → ext
  ./scripts/publish.sh all          # 同上
  ./scripts/publish.sh core         # 仅核心
  ./scripts/publish.sh standard     # 仅标准（请先保证 core 已发布）
  ./scripts/publish.sh ext          # 仅扩展（请先保证 core + standard 已发布）
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
    # 最后一批仍等待，避免紧接着发 standard 时索引未更新
    publish_batch "core/C3 (cache/request)" "${CORE_C3[@]}"
    echo "######## CORE done ########"
}

publish_standard() {
    echo "######## STANDARD ########"
    publish_batch "standard/S1 (model/infra)" "${STANDARD_S1[@]}"
    publish_batch "standard/S2 (model-builtin)" "${STANDARD_S2[@]}"
    publish_batch "standard/S3 (session/auth)" "${STANDARD_S3[@]}"
    publish_batch "standard/S4 (middleware)" "${STANDARD_S4[@]}"
    # 最后一批仍等待，避免紧接着发 ext 时索引未更新
    publish_batch "standard/S5 (routers)" "${STANDARD_S5[@]}"
    echo "######## STANDARD done ########"
}

publish_ext() {
    echo "######## EXTENSION ########"
    publish_batch "ext/E1" "${EXT_E1[@]}"
    publish_batch "ext/E2" "${EXT_E2[@]}"
    echo "######## EXTENSION done ########"
}

list_known() {
    printf '%s\n' \
        "${CORE_C1[@]}" "${CORE_C2[@]}" "${CORE_C3[@]}" \
        "${STANDARD_S1[@]}" "${STANDARD_S2[@]}" "${STANDARD_S3[@]}" \
        "${STANDARD_S4[@]}" "${STANDARD_S5[@]}" \
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
        publish_standard
        publish_ext
        echo "all core + standard + extension crates published successfully"
        ;;
    core)
        publish_core
        ;;
    standard | std)
        publish_standard
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
