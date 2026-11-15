#!/bin/bash
# 按「核心 / 标准」两层发布 tibba-* 到 crates.io（14 个 crate）。
# 分层说明见 docs/crates.md；各 crate README 中有 **分层** 标记。
set -euo pipefail

# ── 核心（Core, 7 crates）：最小脚手架底座，按依赖 DAG 分批 ──────────────
# Batch C1 — 叶子节点，无内部依赖
CORE_C1=(
    tibba-error
    tibba-config
)
# Batch C2 — 依赖 error
CORE_C2=(
    tibba-util
    tibba-crypto
    tibba-runtime
)
# Batch C3 — 依赖 config / util / error（缓存与出站 HTTP 客户端）
CORE_C3=(
    tibba-cache
    tibba-request
)

# ── 标准（Standard, 7 crates）：标准 REST 构件，依赖核心 ─────────────────
# Batch S1 — 基础设施与数据模型
STANDARD_S1=(
    tibba-model
    tibba-job
    tibba-opendal
    tibba-email
)
# Batch S2 — 认证授权合集（依赖 cache / crypto / request / runtime / config）
STANDARD_S2=(
    tibba-auth
)
# Batch S3 — HTTP 中间件栈（依赖 auth / cache / runtime / util）
STANDARD_S3=(
    tibba-middleware
)
# Batch S4 — 统一业务路由聚合层
STANDARD_S4=(
    tibba-router
)

# 索引刷新等待（秒）；可用环境变量覆盖
WAIT_SECS="${TIBBA_PUBLISH_WAIT:-30}"

usage() {
    cat <<'EOF'
用法:
  ./scripts/publish.sh              # 发布全部：core → standard
  ./scripts/publish.sh all          # 同上
  ./scripts/publish.sh core         # 仅核心（7 crates）
  ./scripts/publish.sh standard     # 仅标准（7 crates，请先保证 core 已发布）
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
    publish_batch "core/C1 (error/config)" "${CORE_C1[@]}"
    publish_batch "core/C2 (util/crypto/runtime)" "${CORE_C2[@]}"
    # 最后一批仍等待，避免紧接着发 standard 时索引未更新
    publish_batch "core/C3 (cache/request)" "${CORE_C3[@]}"
    echo "######## CORE done ########"
}

publish_standard() {
    echo "######## STANDARD ########"
    publish_batch "standard/S1 (model/job/opendal/email)" "${STANDARD_S1[@]}"
    publish_batch "standard/S2 (auth)" "${STANDARD_S2[@]}"
    publish_batch "standard/S3 (middleware)" "${STANDARD_S3[@]}"
    publish_batch "standard/S4 (router)" "${STANDARD_S4[@]}"
    echo "######## STANDARD done ########"
}

list_known() {
    printf '%s\n' \
        "${CORE_C1[@]}" "${CORE_C2[@]}" "${CORE_C3[@]}" \
        "${STANDARD_S1[@]}" "${STANDARD_S2[@]}" "${STANDARD_S3[@]}" "${STANDARD_S4[@]}"
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
        echo "all core + standard crates published successfully"
        ;;
    core)
        publish_core
        ;;
    standard | std)
        publish_standard
        ;;
    tibba-*)
        # 单包：必须在清单内（防止拼错包名时静默失败）
        if ! list_known | grep -qx "$cmd"; then
            echo "error: unknown or non-publishable crate: $cmd" >&2
            echo "hint: 清单见 docs/crates.md" >&2
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
