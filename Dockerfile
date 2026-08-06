# syntax=docker/dockerfile:1

# ── Frontend ──────────────────────────────────────────────
FROM node:22-bookworm-slim AS frontend
WORKDIR /web
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# ── Rust binaries ─────────────────────────────────────────
# Build with a live Postgres for sqlx query! macros, or set SQLX_OFFLINE=true
# when .sqlx/ is committed. Default: compile against build-time DATABASE_URL.
FROM rust:1.85-bookworm AS rust
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Cache deps
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/crates/core/Cargo.toml crates/core/Cargo.toml
COPY backend/crates/api/Cargo.toml crates/api/Cargo.toml
COPY backend/crates/collector/Cargo.toml crates/collector/Cargo.toml
COPY backend/crates/admin/Cargo.toml crates/admin/Cargo.toml
RUN mkdir -p crates/core/src crates/api/src crates/collector/src crates/admin/src \
    && echo "pub fn _x(){}" > crates/core/src/lib.rs \
    && echo "fn main(){}" > crates/api/src/main.rs \
    && echo "pub fn _x(){}" > crates/api/src/lib.rs \
    && echo "fn main(){}" > crates/collector/src/main.rs \
    && echo "fn main(){}" > crates/admin/src/main.rs \
    && cargo build --release -p ght-api -p ght-collector -p ght-admin || true

COPY backend/ ./
# sqlx offline cache if present
COPY backend/.sqlx ./.sqlx
ENV SQLX_OFFLINE=true
ARG DATABASE_URL=postgres://ght:ght@localhost:5432/ghtrending
ENV DATABASE_URL=${DATABASE_URL}
RUN cargo build --release -p ght-api -p ght-collector -p ght-admin \
    && strip target/release/ght-api target/release/ght-collector target/release/ght-admin

# ── Runtime ───────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -u 10001 -m ght
WORKDIR /app
COPY --from=rust /src/target/release/ght-api /usr/local/bin/ght-api
COPY --from=rust /src/target/release/ght-collector /usr/local/bin/ght-collector
COPY --from=rust /src/target/release/ght-admin /usr/local/bin/ght-admin
COPY --from=rust /src/migrations /app/migrations
COPY --from=frontend /web/dist /app/frontend/dist
ENV STATIC_DIR=/app/frontend/dist
USER ght
EXPOSE 8000
CMD ["ght-api"]
