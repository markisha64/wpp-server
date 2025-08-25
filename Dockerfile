# Stage 1: Build the Rust application
FROM rust:1.85-bookworm AS build

# Set the working directory inside the container
WORKDIR /wpp-server

# missing deps
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    build-essential \
    python3 \
    python3-pip \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt

# Copy the rest of the application source code
COPY src ./src
COPY shared ./shared
COPY Cargo.lock Cargo.toml ./

# Fetch dependencies to cache them
RUN cargo fetch

# Build the release version of the application
RUN cargo build --release

# Stage 2: Create minimal runtime image
FROM debian:bookworm-slim

# Install runtime OpenSSL (needed by your binary at runtime)
RUN apt-get update && apt-get install -y \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=build /wpp-server/target/release/wpp-server .

RUN mkdir media

CMD ["./wpp-server"]
