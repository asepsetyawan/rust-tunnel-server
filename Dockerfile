FROM rust:1-slim-bookworm AS build

WORKDIR /app
RUN apt-get update \
    && apt-get install --no-install-recommends -y pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY cli ./cli
COPY client ./client
COPY server ./server

ENV OPENSSL_STATIC=1 \
    OPENSSL_NO_VENDOR=1

RUN cargo build --locked --release -p localtunnel

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /app/target/release/localtunnel /usr/local/bin/localtunnel

USER nobody:nogroup
WORKDIR /tmp

ENTRYPOINT ["/usr/local/bin/localtunnel"]
CMD ["--help"]
