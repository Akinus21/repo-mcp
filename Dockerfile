FROM rust:1-bookworm AS builder
WORKDIR /build

# Cache dependencies separately from source changes
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release || true

COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim

# Install ca-certificates FIRST so curl can verify HTTPS
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl gnupg2 \
    && rm -rf /var/lib/apt/lists/*

# Download GitHub CLI GPG key and set up signed APT source
RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
        -o /usr/share/keyrings/githubcli-archive-keyring.gpg \
    && chmod go+r /usr/share/keyrings/githubcli-archive-keyring.gpg \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages" \
        > /etc/apt/sources.list.d/github-cli.list

# Update with the new source, then install all tools in one layer
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        git ca-certificates gh \
        curl wget ripgrep tree \
        openssh-client git-lfs \
        findutils coreutils jq \
    && git lfs install \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/repo-mcp /usr/local/bin/repo-mcp

ENV REPO_MCP_BASE_DIR=/repos
ENV REPO_MCP_PORT=8080
RUN mkdir -p /repos

EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/repo-mcp"]
