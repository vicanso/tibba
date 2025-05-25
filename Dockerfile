FROM rust:1.86.0 AS builder

COPY . /tibba

RUN apt update \
    && apt install -y cmake ca-certificates --no-install-recommends
RUN rustup target list --installed
RUN cd /tibba \
    && make release \
    && ls -lh target/release

FROM ubuntu:24.04

COPY --from=builder /etc/ssl /etc/ssl
COPY --from=builder /tibba/target/release/tibba-api /usr/local/bin/tibba-api


CMD ["tibba-api"]

ENTRYPOINT ["/entrypoint.sh"]