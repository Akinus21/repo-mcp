FROM rust:1-bookworm AS builder
WORKDIR /build

# Cache dependencies separately from source changes
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release || true

COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/repo-mcp /usr/local/bin/repo-mcp

ENV REPO_MCP_BASE_DIR=/repos
ENV REPO_MCP_PORT=8080
RUN mkdir -p /repos

EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/repo-mcp"]
