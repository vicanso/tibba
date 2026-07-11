# syntax=docker/dockerfile:1
# 多阶段：admin SPA → Rust 编译 → debian-slim 运行时（小于完整 ubuntu）

FROM node:24-alpine AS webbuilder
WORKDIR /tibba/admin
COPY admin/package.json admin/package-lock.json ./
RUN npm ci --ignore-scripts
COPY admin/ ./
RUN npm run build

FROM rust:1.95.0 AS builder
ARG GIT_COMMIT_ID
WORKDIR /tibba

RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake ca-certificates nasm curl \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL "https://github.com/vicanso/http-stat-rs/releases/latest/download/httpstat-linux-musl-$(uname -m).tar.gz" \
    | tar -xzf - \
    && mv httpstat /usr/local/bin/

# 含 admin/dist（来自 webbuilder）与完整源码
COPY --from=webbuilder /tibba/admin/dist /tibba/admin/dist
COPY . .
# webbuilder 的 dist 覆盖 COPY . 可能带来的空 dist
COPY --from=webbuilder /tibba/admin/dist /tibba/admin/dist

RUN echo "$GIT_COMMIT_ID" | cut -c1-7 > configs/commit_id.txt \
    && make release \
    && ls -lh target/release/tibba \
    && strip target/release/tibba || true

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /home/tibba --create-home tibba

COPY --from=builder /tibba/target/release/tibba /usr/local/bin/tibba
COPY --from=builder /tibba/entrypoint.sh /entrypoint.sh
COPY --from=builder /usr/local/bin/httpstat /usr/local/bin/httpstat
RUN chmod +x /entrypoint.sh /usr/local/bin/tibba

USER tibba
ENTRYPOINT ["/entrypoint.sh"]
CMD ["tibba"]
