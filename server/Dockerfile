# multi-stage Rust 빌드 (workspace: server + server/migration)
# Build context is the repo root (see docker-compose.yml).

# Stage 1: builder
FROM rust:1.95-slim AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 의존성 캐시 레이어: workspace + 두 멤버 크레이트의 manifest 만 먼저 복사
COPY Cargo.toml Cargo.lock ./
COPY server/Cargo.toml server/Cargo.toml
COPY server/migration/Cargo.toml server/migration/Cargo.toml

# 더미 소스로 의존성만 미리 빌드 — finance-manager (lib+bin) + migration (lib+bin)
RUN mkdir -p server/src server/src/bin server/migration/src \
 && echo "fn main() {}"            > server/src/main.rs \
 && echo ""                        > server/src/lib.rs \
 && echo "fn main() {}"            > server/src/bin/test_import.rs \
 && echo "fn main() {}"            > server/migration/src/main.rs \
 && echo ""                        > server/migration/src/lib.rs \
 && cargo build --release -p finance-manager 2>&1 | tail -5 \
 && rm -rf server/src server/migration/src

# 실제 소스 복사
COPY server/src server/src
COPY server/migration/src server/migration/src

# 타임스탬프 갱신 후 본 빌드
RUN touch server/src/main.rs server/src/lib.rs server/migration/src/lib.rs \
 && cargo build --release -p finance-manager

# Stage 2: runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/finance-manager ./finance-manager

EXPOSE 8000

CMD ["./finance-manager"]
