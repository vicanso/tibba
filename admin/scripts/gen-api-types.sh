#!/usr/bin/env bash
# 从后端 OpenAPI 生成 TypeScript 类型（需 Node + 可选 openapi-typescript）。
# 用法：
#   1) 在仓库根：cargo run --bin export-openapi -- admin/openapi.json
#   2) cd admin && ./scripts/gen-api-types.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SPEC="${1:-$ROOT/openapi.json}"
OUT="${2:-$ROOT/src/api/schema.d.ts}"

if [[ ! -f "$SPEC" ]]; then
  echo "missing OpenAPI spec: $SPEC" >&2
  echo "run from repo root: cargo run --bin export-openapi -- admin/openapi.json" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUT")"
if command -v npx >/dev/null 2>&1; then
  npx --yes openapi-typescript "$SPEC" -o "$OUT"
  echo "wrote $OUT"
else
  echo "npx not found; install Node.js or run openapi-typescript manually" >&2
  exit 1
fi
