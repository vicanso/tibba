FROM rust:1.86.0 AS builder

# Accept GIT_COMMIT_ID as build argument
ARG GIT_COMMIT_ID

COPY . /tibba

# Write the GIT_COMMIT_ID to configs/commit_id.txt
RUN echo "$GIT_COMMIT_ID" > /tibba/configs/commit_id.txt

RUN apt update \
    && apt install -y cmake ca-certificates nasm curl --no-install-recommends
RUN rustup target list --installed
RUN curl -L https://github.com/vicanso/http-stat-rs/releases/latest/download/httpstat-linux-musl-$(uname -m).tar.gz | tar -xzf -
RUN mv httpstat /usr/local/bin/

RUN cd /tibba \
    && cat configs/commit_id.txt \
    && make release \
    && ls -lh target/release

FROM ubuntu:24.04

COPY --from=builder /etc/ssl /etc/ssl
COPY --from=builder /tibba/target/release/tibba /usr/local/bin/tibba
COPY --from=builder /tibba/entrypoint.sh /entrypoint.sh
COPY --from=builder /usr/local/bin/httpstat /usr/local/bin/httpstat

CMD ["tibba"]

ENTRYPOINT ["/entrypoint.sh"]