# multi-stage Rust 빌드
# Stage 1: builder
FROM rust:1.95-slim AS builder

# 시스템 의존성 설치
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 의존성 캐시 레이어 (소스 변경 시에도 의존성은 재사용)
COPY Cargo.toml Cargo.lock ./
# 더미 main.rs로 의존성만 먼저 빌드
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release 2>&1 | tail -5
RUN rm -rf src

# 실제 소스 복사 및 빌드
COPY src ./src
COPY migrations ./migrations
COPY .sqlx ./.sqlx
# 소스 변경 타임스탬프 갱신
RUN touch src/main.rs
RUN SQLX_OFFLINE=true cargo build --release

# Stage 2: runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/finance-manager ./finance-manager
COPY --from=builder /app/migrations ./migrations

# 포트 노출
EXPOSE 8000

CMD ["./finance-manager"]
