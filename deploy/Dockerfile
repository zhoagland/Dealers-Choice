# Dealers Choice web server: one image with the dc-server binary and the built React app
# (docs/tech/web.md §10). Build from the repo root:
#   docker build -f deploy/Dockerfile -t dealers-choice .

FROM node:22-bookworm-slim AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build

FROM rust:1-bookworm AS server
WORKDIR /src
COPY . .
RUN cargo build --release -p server --locked

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /data dc \
 && mkdir -p /data /app \
 && chown dc /data
COPY --from=server /src/target/release/dc-server /usr/local/bin/dc-server
COPY --from=web /web/dist /app/web
ENV STATIC_DIR=/app/web \
    DC_DATA_ROOT=/data \
    BIND_ADDR=0.0.0.0:8080
USER dc
VOLUME /data
EXPOSE 8080
CMD ["dc-server"]
