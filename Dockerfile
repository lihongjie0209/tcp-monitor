FROM rust:1.88-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config \
    python3 \
    && rm -rf /var/lib/apt/lists/*

# Cache dependencies
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo 'fn main(){}' > src/main.rs && cargo build 2>/dev/null; rm -f src/main.rs

# Build and test
COPY src ./src
COPY test_connections.py ./
RUN cargo build 2>&1
RUN cargo test -- --nocapture 2>&1

CMD ["./target/debug/tcp-monitor"]
