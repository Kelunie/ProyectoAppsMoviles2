# Build stage
FROM rust:slim-bookworm AS builder
WORKDIR /app

# Cache dependencies first
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release || true

# Build the real binary
COPY src ./src
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim
WORKDIR /app

# Needed for TLS/DNS when connecting to MongoDB Atlas
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/virus_game_server /app/server

EXPOSE 3000
CMD ["./server"]
