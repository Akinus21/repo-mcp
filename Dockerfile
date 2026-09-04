FROM rust:1-bookworm AS builder
WORKDIR /build

# Cache dependencies separately from source changes
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release || true

COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim

# Install base tools in one layer
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl git \
        wget ripgrep tree \
        openssh-client git-lfs \
        findutils coreutils jq \
    && git lfs install \
    && rm -rf /var/lib/apt/lists/*

# Install GitHub CLI from official binary release (avoids apt source issues)
ARG GH_VERSION="2.63.2"
RUN curl -fsSL "https://github.com/cli/cli/releases/download/v${GH_VERSION}/gh_${GH_VERSION}_linux_amd64.tar.gz" \
        -o /tmp/gh.tar.gz \
    && tar -xzf /tmp/gh.tar.gz -C /tmp \
    && mv /tmp/gh_${GH_VERSION}_linux_amd64/bin/gh /usr/local/bin/ \
    && rm -rf /tmp/gh.tar.gz /tmp/gh_${GH_VERSION}_linux_amd64

COPY --from=builder /build/target/release/repo-mcp /usr/local/bin/repo-mcp

ENV REPO_MCP_BASE_DIR=/repos
ENV REPO_MCP_PORT=8080
RUN mkdir -p /repos

EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/repo-mcp"]
