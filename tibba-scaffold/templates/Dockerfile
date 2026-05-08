FROM node:24-alpine AS webbuilder

COPY . /{{name}}
RUN apk update \
    && apk add git make \
    && cd /{{name}}/admin \
    && npm install --force \
    && npm run build


FROM rust:1.95.0 AS builder

# Accept GIT_COMMIT_ID as build argument
ARG GIT_COMMIT_ID

COPY --from=webbuilder /{{name}} /{{name}}

# Write the GIT_COMMIT_ID to configs/commit_id.txt
RUN echo "$GIT_COMMIT_ID" | cut -c1-7 > /{{name}}/configs/commit_id.txt

RUN apt update \
    && apt install -y cmake ca-certificates nasm curl --no-install-recommends

RUN cd /{{name}} \
    && cat configs/commit_id.txt \
    && make release \
    && ls -lh target/release

FROM ubuntu:24.04

COPY --from=builder /etc/ssl /etc/ssl
COPY --from=builder /{{name}}/target/release/{{name}} /usr/local/bin/{{name}}
COPY --from=builder /{{name}}/entrypoint.sh /entrypoint.sh

CMD ["{{name}}"]

ENTRYPOINT ["/entrypoint.sh"]
