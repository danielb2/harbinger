# syntax=docker/dockerfile:1.7

FROM --platform=$BUILDPLATFORM rust:1-bookworm AS builder
ARG TARGETARCH
WORKDIR /build

# Shared build helper. Keeping target/cross setup in one place means the
# dependency layer and the application layer build in exactly the same way.
RUN <<'EOF'
cat > /usr/local/bin/harbinger-build <<'SCRIPT'
#!/bin/sh
set -eu

target_arch="${TARGETARCH:-$(dpkg --print-architecture)}"

case "$target_arch" in
    amd64)
        rust_target=x86_64-unknown-linux-gnu
        cross_package=gcc-x86-64-linux-gnu
        cross_libc_dev=libc6-dev-amd64-cross
        cross_cc=x86_64-linux-gnu-gcc
        cross_ar=x86_64-linux-gnu-ar
        linker_var=CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER
        cc_var=CC_x86_64_unknown_linux_gnu
        ar_var=AR_x86_64_unknown_linux_gnu
        ;;
    arm64)
        rust_target=aarch64-unknown-linux-gnu
        cross_package=gcc-aarch64-linux-gnu
        cross_libc_dev=libc6-dev-arm64-cross
        cross_cc=aarch64-linux-gnu-gcc
        cross_ar=aarch64-linux-gnu-ar
        linker_var=CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER
        cc_var=CC_aarch64_unknown_linux_gnu
        ar_var=AR_aarch64_unknown_linux_gnu
        ;;
    *)
        echo "Unsupported target architecture: $target_arch" >&2
        exit 1
        ;;
esac

case "${1:-}" in
    setup)
        rustup target add "$rust_target"
        if [ "$(dpkg --print-architecture)" != "$target_arch" ]; then
            apt-get update
            apt-get install -y --no-install-recommends "$cross_package" "$cross_libc_dev"
            rm -rf /var/lib/apt/lists/*
        fi
        ;;
    build)
        if [ "$(dpkg --print-architecture)" != "$target_arch" ]; then
            export "$linker_var=$cross_cc"
            export "$cc_var=$cross_cc"
            export "$ar_var=$cross_ar"
        fi
        cargo build --release --target "$rust_target"
        cp "target/$rust_target/release/harbinger" /build/harbinger
        ;;
    *)
        echo "usage: harbinger-build setup|build" >&2
        exit 2
        ;;
esac
SCRIPT
chmod +x /usr/local/bin/harbinger-build
EOF

RUN harbinger-build setup

# Dependency layer: compile Cargo dependencies once with a dummy main. Source
# changes only invalidate the final application build below.
COPY Cargo.toml Cargo.lock ./
COPY build.rs ./
RUN mkdir -p src && printf 'fn main() {}\n' > src/main.rs
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target,sharing=locked \
    harbinger-build build
RUN rm -rf src

# Keep the commit after the dependency layer so changing it does not
# invalidate the compiled dependency cache.
ARG GIT_COMMIT=unknown
ENV HARBINGER_GIT_COMMIT=$GIT_COMMIT

COPY assets ./assets
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target,sharing=locked \
    harbinger-build build

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
ARG IMAGE_BUILD_DATE=unknown
ENV IMAGE_BUILD_DATE=${IMAGE_BUILD_DATE}
COPY --from=builder /build/harbinger /usr/local/bin/harbinger
RUN mkdir -p /config
VOLUME ["/config", "/media"]
EXPOSE 32400/tcp 4826/tcp
ENTRYPOINT ["harbinger", "--config", "/config", "--bind", "0.0.0.0:32400"]
