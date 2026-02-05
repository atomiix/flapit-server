ARG RUST_VERSION=1.93
ARG ALPINE_VERSION=3.22
ARG APP_NAME=flapit_server

FROM docker.io/library/rust:${RUST_VERSION}-alpine AS build
ARG APP_NAME
WORKDIR /app

RUN --mount=type=bind,source=src,target=src \
    --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
    --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=cache,target=/app/target/ \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
    cargo build --locked --release && \
    cp ./target/release/$APP_NAME /bin/

FROM scratch

ARG APP_NAME

COPY --from=build /bin/${APP_NAME} /bin/

EXPOSE 3000
EXPOSE 443

ENTRYPOINT ["/bin/flapit_server"]