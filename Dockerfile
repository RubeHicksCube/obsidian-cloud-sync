FROM rust:slim AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Cache dependencies by building a dummy project first
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src

# Build the actual application
COPY src/ src/
COPY migrations/ migrations/
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/obsidian-cloud-sync /usr/local/bin/obsidian-cloud-sync
EXPOSE 8443
ENV BIND_ADDRESS=0.0.0.0:8443
ENV DATABASE_URL=sqlite:data/obsidian_sync.db
ENV DATA_DIR=data
VOLUME ["/data"]
CMD ["obsidian-cloud-sync"]
