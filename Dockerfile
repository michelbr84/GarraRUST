# Stage 1: Build from local source
FROM rust:1.86-slim AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev curl && rm -rf /var/lib/apt/lists/*

# Install Node.js LTS (required for npx-based MCP servers: n8n-mcp, filesystem-mcp, etc.)
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependencies — copy manifests first
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

# Build release binary with HTTP/SSE MCP transport support
RUN cargo build --release --features mcp-http

# Stage 2: Minimal runtime image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*

# Install Node.js in runtime image (needed to spawn npx MCP servers at runtime)
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/garraia /usr/local/bin/garraia

RUN useradd -m -s /bin/bash garraia
USER garraia
WORKDIR /home/garraia

EXPOSE 3888

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl -sf http://localhost:3888/health || exit 1

ENTRYPOINT ["garraia"]
CMD ["start", "--host", "0.0.0.0"]
