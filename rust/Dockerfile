# Base image extends rust:nightly which extends debian:buster-slim
FROM rustlang/rust:nightly as build

# Install necessary build dependencies, including protoc
RUN apt-get update && apt-get install -y \
    libssl-dev \
    libsqlite3-dev \
    libudev-dev \
    pkg-config \
    protobuf-compiler && \
    apt-get clean && rm -rf /var/lib/apt/lists/*

# Compile Lit Node
RUN rm -rf /tmp/lit_node
RUN mkdir -p /tmp/lit_node

COPY lit-actions/ /tmp/lit-actions/
COPY lit-core/ /tmp/lit-core/
COPY lit-os/ /tmp/lit-os/
COPY lit-node/ /tmp/lit-node/

WORKDIR /tmp/lit-node

RUN cargo build --release

# Copy the binary into a new container for a smaller docker image
FROM ubuntu:22.04

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libsqlite3-dev build-essential autoconf libtool curl git libssl-dev libudev-dev pkg-config ltrace strace cmake protobuf-compiler && \
    apt-get clean && rm -rf /var/lib/apt/lists/*

COPY --from=build /tmp/lit-node/target/release/lit_node /
USER root

ENV RUST_LOG=info
ENV RUST_BACKTRACE=full
ENV LIT_NODE_PORT=7470
ENV ROCKET_ADDRESS=0.0.0.0
ENV LIT_NODE_DOMAIN_NAME=node3.litgateway.com

EXPOSE 7470
ENV PORT 7470

CMD ["/lit_node"]
