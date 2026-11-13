# The central server (master + web UI) in a container. Nodes are not
# containerised by design: they need the host (WireGuard, firewall, the
# user's files), so this image runs no agentic node (HARNESS_NO_LOCAL_NODE).
# Debian 13 base: the master uploads its own binary to nodes, so its glibc
# must not be newer than the nodes' (Debian 13, Ubuntu 26.04).
# REGISTRY: e.g. mirror.gcr.io/library when Docker Hub rate-limits you.

ARG REGISTRY=docker.io/library
FROM ${REGISTRY}/node:22-trixie-slim AS web
WORKDIR /src/web
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml ./
# pnpm at the version package.json pins (packageManager).
RUN npm install --global --no-audit --no-fund "$(node -p 'require("./package.json").packageManager')" \
 && pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

FROM ${REGISTRY}/rust:1.99-slim-trixie AS build
WORKDIR /src
COPY . .
COPY --from=web /src/web/dist web/dist
RUN cargo build --release --locked -p harness \
 && install -m 0755 target/release/rizzy-harness /rizzy-harness

FROM ${REGISTRY}/debian:trixie-slim
# No apt: the only extra file needed is the CA bundle (provider APIs, web tools).
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
RUN useradd --system --uid 10001 --home-dir /data --shell /usr/sbin/nologin rizzy-harness \
 && mkdir /data && chown rizzy-harness:rizzy-harness /data && chmod 0700 /data
COPY --from=build /rizzy-harness /usr/local/bin/rizzy-harness
USER rizzy-harness
ENV HARNESS_DATA_DIR=/data \
    HARNESS_CONTAINER=1 \
    HARNESS_NO_LOCAL_NODE=1
VOLUME /data
EXPOSE 7777
# serve stops cleanly on SIGTERM and SIGINT alike (A.19): chat turns are
# closed, then HTTP ends; give it time (`docker stop -t 150`, compose
# `stop_grace_period: 150s`): Docker's default 10 s can cut a turn's close.
STOPSIGNAL SIGINT
ENTRYPOINT ["rizzy-harness"]
CMD ["serve", "--bind", "0.0.0.0:7777"]
