# ============================================================================
# GarraIA — Multi-stage Dockerfile
# Target: < 50MB final image with Node.js 22 for MCP servers
# ============================================================================

# ---------------------------------------------------------------------------
# Stage 1: Chef — prepare recipe for dependency caching
# ---------------------------------------------------------------------------
FROM rust:1.86-slim AS chef

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

RUN cargo install cargo-chef --locked
WORKDIR /build

# ---------------------------------------------------------------------------
# Stage 2: Planner — generate build recipe from source
# ---------------------------------------------------------------------------
FROM chef AS planner

COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo chef prepare --recipe-path recipe.json

# ---------------------------------------------------------------------------
# Stage 3: Builder — compile release binary with cached deps
# ---------------------------------------------------------------------------
FROM chef AS builder

COPY --from=planner /build/recipe.json recipe.json

# Build dependencies only (cached layer)
RUN cargo chef cook --release --recipe-path recipe.json

# Copy full source and build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release --package garraia \
    && strip target/release/garra

# ---------------------------------------------------------------------------
# Stage 4: Node — install Node.js 22 minimal for MCP servers
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS node-installer

RUN apt-get update && apt-get install -y --no-install-recommends \
        curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/* \
    && npm cache clean --force

# ---------------------------------------------------------------------------
# Stage 5: Runtime — minimal production image
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

LABEL org.opencontainers.image.title="GarraIA Gateway" \
      org.opencontainers.image.description="Multi-channel, multi-provider LLM orchestration gateway" \
      org.opencontainers.image.vendor="michelbr84" \
      org.opencontainers.image.source="https://github.com/michelbr84/GarraRUST" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.version="0.3.0"

# Install only essential runtime deps
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl tini \
    && rm -rf /var/lib/apt/lists/*

# Copy Node.js from installer stage
COPY --from=node-installer /usr/bin/node /usr/bin/node
COPY --from=node-installer /usr/bin/npx /usr/bin/npx
COPY --from=node-installer /usr/bin/npm /usr/bin/npm
COPY --from=node-installer /usr/lib/node_modules /usr/lib/node_modules

# Copy compiled binary
COPY --from=builder /build/target/release/garra /usr/local/bin/garra

# Create non-root user
RUN groupadd --gid 1000 garraia \
    && useradd --uid 1000 --gid 1000 --create-home --shell /bin/bash garraia \
    && mkdir -p /home/garraia/.config/garraia/data \
                /home/garraia/.config/garraia/credentials \
                /home/garraia/.config/garraia/skills \
    && chown -R garraia:garraia /home/garraia

USER garraia
WORKDIR /home/garraia

EXPOSE 3888

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -sf http://localhost:3888/health || exit 1

ENTRYPOINT ["tini", "--", "garra"]
CMD ["start", "--host", "0.0.0.0"]
