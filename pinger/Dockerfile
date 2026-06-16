# syntax=docker/dockerfile:1

FROM rust:1.93-bookworm AS builder

WORKDIR /app

## Install dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    clang \
    llvm-dev \
    libclang-dev \
    libbz2-dev \
    liblz4-dev \
    libsnappy-dev \
    libzstd-dev \
    zlib1g-dev \
    pkg-config \
    cmake \
    curl \
    unzip \
    git \
    && rm -rf /var/lib/apt/lists/*

ARG PROTOC_VERSION=34.0
ARG PROTOC_SHA256=e9a91b6fcfe4177ec2cd35fc8f15c1e811fa0ecdef9372755cd6d3513d5faaab
RUN PROTOC_ZIP="protoc-${PROTOC_VERSION}-linux-x86_64.zip" \
   && curl -fsSLO "https://github.com/protocolbuffers/protobuf/releases/download/v${PROTOC_VERSION}/${PROTOC_ZIP}" \
   && echo "${PROTOC_SHA256}  ${PROTOC_ZIP}" | sha256sum -c - \
   && unzip -o $PROTOC_ZIP -d /usr/local bin/protoc \
   && unzip -o $PROTOC_ZIP -d /usr/local 'include/*' \
   && rm -f $PROTOC_ZIP

# Copy manifests and lock file first for better layer caching
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./

# Copy workspace members so Cargo can load the workspace while building pinger.
COPY contract ./contract
COPY tests ./tests

# Copy source code
COPY pinger ./pinger

RUN cargo build -p liquid-staking-token-pinger --release

# ---- Runtime Stage ----
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libbz2-1.0 \
    liblz4-1 \
    libsnappy1v5 \
    libzstd1 \
    zlib1g \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r pinger && useradd -r -g pinger pinger

WORKDIR /app

# Copy the compiled binary from the builder stage
COPY --from=builder /app/target/release/liquid-staking-token-pinger /app/liquid-staking-token-pinger

# Copy the default config. Operators can override by bind-mounting their own file at /app/pinger.yml.
COPY pinger/pinger.yml /app/pinger.yml

# Change ownership and switch user
RUN chown -R pinger:pinger /app
USER pinger

# Create mount points to the directories where the pinger will read/write data
VOLUME ["/app/data"]

ENTRYPOINT ["/app/liquid-staking-token-pinger"]
CMD ["-c", "/app/pinger.yml"]
