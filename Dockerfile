# The central server (master + web UI) in a container. Nodes are not
# containerised by design: they need the host (WireGuard, firewall, the
# user's files), so this image runs no agentic node (HARNESS_NO_LOCAL_NODE).
# Debian 13 base: the master uploads its own binary to nodes, so its glibc
# must not be newer than the nodes' (Debian 13, Ubuntu 26.04).
# REGISTRY: e.g. mirror.gcr.io/library when Docker Hub rate-limits you.

ARG REGISTRY=docker.io/library
FROM ${REGISTRY}/node:22-trixie-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

FROM ${REGISTRY}/rust:1.99-slim-trixie AS build
WORKDIR /src
COPY . .
COPY --from=web /src/web/dist web/dist
RUN cargo build --release --locked -p harness \
 && install -m 0755 target/release/harness /harness

FROM ${REGISTRY}/debian:trixie-slim
# No apt: the only extra file needed is the CA bundle (provider APIs, web tools).
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
RUN useradd --system --uid 10001 --home-dir /data --shell /usr/sbin/nologin harness \
 && mkdir /data && chown harness:harness /data && chmod 0700 /data
COPY --from=build /harness /usr/local/bin/harness
USER harness
ENV HARNESS_DATA_DIR=/data \
    HARNESS_CONTAINER=1 \
    HARNESS_NO_LOCAL_NODE=1
VOLUME /data
EXPOSE 7777
# serve shuts down cleanly on SIGINT; PID 1 would ignore SIGTERM.
STOPSIGNAL SIGINT
ENTRYPOINT ["harness"]
CMD ["serve", "--bind", "0.0.0.0:7777"]
