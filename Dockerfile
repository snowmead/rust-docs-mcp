# Multi-stage Dockerfile for rust-docs-mcp
# Supports multi-architecture builds (x86_64, ARM64)

# Build stage
FROM rust:1.75-slim as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    git \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Install nightly toolchain with required components
RUN rustup toolchain install nightly \
    && rustup component add --toolchain nightly rustfmt clippy rust-src

# Set working directory
WORKDIR /app

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY rust-toolchain.toml ./

# Copy source code
COPY rust-docs-mcp/ ./rust-docs-mcp/
COPY cargo-modules/ ./cargo-modules/

# Build the application in release mode
RUN cargo build --release -p rust-docs-mcp

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    git \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -s /bin/bash rustdocs

# Copy binary from builder stage
COPY --from=builder /app/target/release/rust-docs-mcp /usr/local/bin/rust-docs-mcp

# Set up cache directory with proper permissions
RUN mkdir -p /home/rustdocs/.rust-docs-mcp/cache && \
    chown -R rustdocs:rustdocs /home/rustdocs/.rust-docs-mcp

# Switch to non-root user
USER rustdocs
WORKDIR /home/rustdocs

# Set environment variables
ENV RUST_DOCS_MCP_CACHE_DIR=/home/rustdocs/.rust-docs-mcp/cache

# Expose MCP stdio interface
ENTRYPOINT ["rust-docs-mcp"]