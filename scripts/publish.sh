#!/bin/bash
set -euo pipefail

publish() {
    echo "publishing $1 ..."
    cargo publish --registry crates-io -p "$1"
}

# 指定单个模块时直接发布并退出
if [ "${1:-}" != "" ]; then
    publish "$1"
    exit 0
fi

# Batch 1 — 无依赖
publish tibba-error
publish tibba-state
publish tibba-performance
publish tibba-validator

echo "batch 1 done, waiting for crates.io index..."
sleep 30

# Batch 2 — 仅依赖 error
publish tibba-util
publish tibba-config
publish tibba-crypto
publish tibba-headless
publish tibba-hook
publish tibba-scheduler
publish tibba-model

echo "batch 2 done, waiting for crates.io index..."
sleep 30

# Batch 3 — 依赖 config / error / util
publish tibba-cache
publish tibba-opendal
publish tibba-request
publish tibba-sql

echo "batch 3 done, waiting for crates.io index..."
sleep 30

# Batch 4 — 依赖 cache / state / util
publish tibba-session
publish tibba-middleware
publish tibba-router-common

echo "batch 4 done, waiting for crates.io index..."
sleep 30

# Batch 5 — 依赖 middleware / session / model 等
publish tibba-router-file
publish tibba-router-model
publish tibba-router-user

echo "all crates published successfully"
