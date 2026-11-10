# syntax=docker/dockerfile:1
#
# Settoria in one image: the React client is built with Node, the server with Rust, and the
# result runs on a small Debian image. Build and run it with `docker compose up --build`.

# ---------- 1. the web client -> /app/web/dist ----------
FROM node:22-alpine AS web
WORKDIR /app/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
# The changelog page imports ../docs/Changelog.md.
COPY docs/ /app/docs/
RUN npm run build

# ---------- 2. the server -> /app/target/release/catan-lan ----------
FROM rust:1-slim-bookworm AS builder
WORKDIR /app
# Build the dependencies first, on their own, so editing the code doesn't rebuild them.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src
# The built-in maps and the example map are compiled into the program with include_str!.
COPY maps/ ./maps/
COPY examples/ ./examples/
COPY src/ ./src/
RUN touch src/main.rs && cargo build --release

# ---------- optional: the test suite (`docker compose --profile test run --rm tests`) ----------
FROM builder AS tests
CMD ["cargo", "test", "--release"]

# ---------- 3. what actually runs ----------
FROM debian:bookworm-slim AS runtime
RUN useradd --system --no-create-home --shell /usr/sbin/nologin settoria
WORKDIR /app
COPY --from=builder /app/target/release/catan-lan /usr/local/bin/catan-lan
COPY --from=web /app/web/dist ./web/dist
# Maps written as JSON files are read from here when the server starts (see "Maps" in the README).
COPY maps/ ./maps/
COPY examples/ ./examples/

ENV PORT=3000 \
    STATIC_DIR=/app/web/dist \
    MAPS_DIR=/app/maps
EXPOSE 3000
USER settoria

# Ask the server for the lobby with nothing but bash (the image has no curl).
HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=3 \
  CMD ["bash", "-c", "exec 3<>/dev/tcp/127.0.0.1/${PORT} && printf 'GET /api/lobby HTTP/1.0\\r\\n\\r\\n' >&3 && head -n 1 <&3 | grep -q ' 200 '"]

# `docker run settoria validate some-map.json` checks a map file without starting the server.
ENTRYPOINT ["catan-lan"]
