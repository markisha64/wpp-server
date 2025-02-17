# Stage 1: Build the Rust application
FROM rust:slim as build

# Set the working directory inside the container
WORKDIR /wpp-server

# Copy the rest of the application source code
COPY src ./src
COPY shared ./shared
COPY Cargo.lock Cargo.toml ./

# Fetch dependencies to cache them
RUN cargo fetch

# Build the release version of the application
RUN cargo build --release

FROM rust:slim

COPY --from=build /wpp-server/target/release/wpp-server .

# Set the entry point for the container to run the application
CMD ["./wpp-server"]
