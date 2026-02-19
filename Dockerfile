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
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*

# Non-root user
RUN useradd -r -u 1000 -m cloudsync

COPY --from=builder --chown=cloudsync:cloudsync /app/target/release/obsidian-cloud-sync /usr/local/bin/obsidian-cloud-sync

# Create data directory with correct ownership before switching to non-root
RUN mkdir -p /data && chown cloudsync:cloudsync /data

WORKDIR /app
USER cloudsync
EXPOSE 8443
ENV BIND_ADDRESS=0.0.0.0:8443
ENV DATABASE_URL=sqlite:/data/obsidian_sync.db
ENV DATA_DIR=/data
VOLUME ["/data"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl -f http://localhost:8443/api/health || exit 1

CMD ["obsidian-cloud-sync"]
