# Pinned to the exact toolchain the workspace is tested with (the minimum supported version is
# `rust-version` in Cargo.toml). Update it deliberately, together with CI.
FROM rust:1.100.0-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY apps ./apps
COPY crates ./crates
COPY migrations ./migrations
RUN cargo build --release -p rustchat-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl libssl3 && rm -rf /var/lib/apt/lists/* && useradd --system --uid 10001 --no-create-home rustchat
COPY --from=build /src/target/release/rustchat-server /usr/local/bin/rustchat-server
USER 10001
EXPOSE 8080
ENTRYPOINT ["rustchat-server"]
CMD ["serve"]
