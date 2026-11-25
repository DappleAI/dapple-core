# GPU_TYPE selects which base-* stage to use for the runtime image
# GPU_TYPE options: none (default), amd, nvidia, apple
ARG GPU_TYPE=none

# GPU-specific base images (only need runtime libs, not full dev)
# All images use glibc 2.39+ to match the builder (Debian Trixie / Ubuntu 24.04)
FROM rocm/dev-ubuntu-24.04:6.3 AS base-amd
FROM nvidia/cuda:12.6.0-runtime-ubuntu24.04 AS base-nvidia
FROM debian:trixie-slim AS base-none
FROM debian:trixie-slim AS base-apple

# Build stage - same for all GPU types (CPU build, GPU is runtime-only via ONNX RT)
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app
ENV SQLX_OFFLINE=true

# Install sccache for compilation caching
RUN cargo install sccache --version ^0.8
ENV RUSTC_WRAPPER=sccache SCCACHE_DIR=/sccache

FROM chef AS planner
COPY . .
RUN --mount=type=cache,target=/sccache,sharing=locked \
    cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
ARG BIN=dapple-prompt-api
ARG PROFILE=release
ARG TARGETARCH
# Space-separated cargo features for the app build (empty for most services).
# Feature-gated services (e.g. conduit-oximedia needs `oximedia` for its
# /compose-av + /analyze-audio endpoints) pass this so the Docker image matches
# what hostPath mode builds. Empty ⇒ default features, identical to before.
ARG FEATURES=
# Native build deps for feature-gated services: whisper.cpp (via whisper-rs) needs
# cmake + libclang (bindgen); aws-lc-rs (reqwest rustls in conduit-media-gateway)
# needs cmake + perl. Only actually compiled when FEATURES enables those crates,
# so this is a harmless build-time layer for every other service.
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake clang libclang-dev perl && rm -rf /var/lib/apt/lists/*
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - cached by both cargo-chef AND sccache
# Fix for gemm-common FP16 SIMD on ARM64 (Apple Silicon Docker builds)
# See: https://github.com/sarah-quinones/gemm/issues/31
RUN --mount=type=cache,target=/sccache,sharing=locked \
    EXTRA_FLAGS=""; \
    if [ "$TARGETARCH" = "arm64" ]; then \
      EXTRA_FLAGS="-C target-feature=+fp16"; \
    fi; \
    if [ "$PROFILE" = "dev" ]; then \
      RUSTFLAGS="$EXTRA_FLAGS" cargo chef cook --recipe-path recipe.json; \
    else \
      RUSTFLAGS="$EXTRA_FLAGS" cargo chef cook --release --recipe-path recipe.json; \
    fi
# Build application
COPY . .
RUN --mount=type=cache,target=/sccache,sharing=locked \
    EXTRA_FLAGS=""; \
    if [ "$TARGETARCH" = "arm64" ]; then \
      EXTRA_FLAGS="-C target-feature=+fp16"; \
    fi; \
    FEATURE_ARGS=""; \
    if [ -n "$FEATURES" ]; then \
      FEATURE_ARGS="--features $FEATURES"; \
    fi; \
    RUSTFLAGS="$EXTRA_FLAGS" cargo build --profile ${PROFILE} --bin ${BIN} ${FEATURE_ARGS}
# Cargo maps profile names to directories: dev->debug, release->release
# Create a symlink so the COPY below works regardless of profile
RUN TARGET_DIR=$(if [ "$PROFILE" = "dev" ]; then echo "debug"; else echo "$PROFILE"; fi) && \
    if [ "$TARGET_DIR" != "$PROFILE" ]; then \
      ln -s /app/target/$TARGET_DIR /app/target/$PROFILE; \
    fi

# Runtime stage - select base by GPU_TYPE
ARG GPU_TYPE=none
# runtime-base: everything a service binary needs at runtime EXCEPT the binary.
# hostPath deploys run a locally built binary on top of this stage, and
# deploy_dapple_core.nu builds it (--target runtime-base) as the placeholder
# image when the registry has none yet, so those pods get the same CA bundle,
# OpenSSL and ONNX Runtime as a real image.
FROM base-${GPU_TYPE} AS runtime-base
WORKDIR /app

# Install common dependencies
RUN apt-get update && apt-get install -y ca-certificates openssl wget unzip && rm -rf /var/lib/apt/lists/*

# Install ONNX Runtime shared library based on GPU type
# The ort crate with load-dynamic feature loads libonnxruntime.so at runtime.
#
# microsoft/onnxruntime was removed from GitHub [2026-11], so its release
# tarballs 404. Microsoft still publishes the native libraries as NuGet packages
# (the CPU linux-x64 libonnxruntime.so is byte-identical to the old tarball's).
# A .nupkg is a zip; the libraries live under runtimes/linux-x64/native/.
ARG GPU_TYPE=none
ARG ORT_VERSION=1.23.2
# sha256 of Microsoft.ML.OnnxRuntime.<ORT_VERSION>.nupkg — bump with ORT_VERSION.
ARG ORT_CPU_NUPKG_SHA256=25fe172f7fcdf34f2b5b02c8b997f6b88abc282956849d4af4335af23d0c4e4a

RUN set -ex && \
    if [ "$GPU_TYPE" = "amd" ]; then \
        echo "ONNX Runtime ROCm builds were only published as GitHub release tarballs," >&2; \
        echo "which no longer exist. Provide libonnxruntime.so for ROCm yourself." >&2; \
        exit 1; \
    elif [ "$GPU_TYPE" = "nvidia" ]; then \
        # CUDA ONNX Runtime
        wget -q -O /tmp/ort.nupkg "https://www.nuget.org/api/v2/package/Microsoft.ML.OnnxRuntime.Gpu.Linux/${ORT_VERSION}"; \
    else \
        # CPU-only ONNX Runtime
        wget -q -O /tmp/ort.nupkg "https://www.nuget.org/api/v2/package/Microsoft.ML.OnnxRuntime/${ORT_VERSION}" && \
        echo "${ORT_CPU_NUPKG_SHA256}  /tmp/ort.nupkg" | sha256sum -c -; \
    fi && \
    mkdir -p /opt/onnxruntime/lib && \
    unzip -q -o -j /tmp/ort.nupkg 'runtimes/linux-x64/native/*' -d /opt/onnxruntime/lib && \
    rm /tmp/ort.nupkg

# Set up environment for ONNX Runtime
ENV ORT_DYLIB_PATH=/opt/onnxruntime/lib/libonnxruntime.so
ENV LD_LIBRARY_PATH=/opt/onnxruntime/lib
ENV ONNX_MODEL_PATH=/models

FROM runtime-base AS runtime

# Copy the binary from builder
ARG BIN=dapple-prompt-api
ARG PROFILE=release
COPY --from=builder /app/target/${PROFILE}/${BIN} /usr/local/bin/app

# conduit-media-gateway needs runtime media binaries (ffmpeg + yt-dlp) and a baked
# whisper model. Keyed on $BIN so NO other service's image carries any of it.
ARG WHISPER_MODEL_URL=https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
RUN if [ "$BIN" = "conduit-media-gateway" ]; then \
      apt-get update && apt-get install -y --no-install-recommends ffmpeg fonts-dejavu-core && \
      rm -rf /var/lib/apt/lists/* && \
      wget -q -O /usr/local/bin/yt-dlp https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux && \
      chmod a+rx /usr/local/bin/yt-dlp && \
      mkdir -p /models && \
      wget -q -O /models/ggml-base.en.bin "$WHISPER_MODEL_URL"; \
    fi

# ONNX/whisper models: mounted at runtime via hostPath/PVC for most services (keeps
# image small); conduit-media-gateway bakes its whisper model into /models above.
VOLUME ["/models"]
ENV ONNX_MODEL_PATH=/models

ENTRYPOINT ["/usr/local/bin/app"]
