# Full Android debug build: Rust (cargo-ndk) + UniFFI + Gradle.
# Run from repo root: docker build -f docker/android-ci/Dockerfile .
#
# Requires network during build (Gradle, crates.io, Android SDK).

# Default to amd64 so Rust/cargo-ndk/NDK match typical CI (avoids arm64-linux Docker quirks on Apple Silicon).
FROM --platform=linux/amd64 ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive \
    ANDROID_SDK_ROOT=/opt/android-sdk \
    ANDROID_HOME=/opt/android-sdk \
    RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo

ENV PATH="${CARGO_HOME}/bin:${ANDROID_SDK_ROOT}/cmdline-tools/latest/bin:${ANDROID_SDK_ROOT}/platform-tools:${PATH}"

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        openjdk-17-jdk-headless \
        wget \
        unzip \
        curl \
        ca-certificates \
        git \
        build-essential \
        pkg-config \
        libssl-dev \
        perl \
        clang \
        file \
    && rm -rf /var/lib/apt/lists/*

# Android command-line tools + SDK components (API 34 matches app/build.gradle.kts).
RUN mkdir -p "${ANDROID_SDK_ROOT}/cmdline-tools" \
    && wget -q "https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip" -O /tmp/cmdline.zip \
    && unzip -q /tmp/cmdline.zip -d /tmp \
    && mv /tmp/cmdline-tools "${ANDROID_SDK_ROOT}/cmdline-tools/latest" \
    && rm /tmp/cmdline.zip \
    && yes | sdkmanager --licenses >/dev/null \
    && sdkmanager "platforms;android-34" "build-tools;34.0.0" "ndk;26.1.10909125"

ENV ANDROID_NDK_HOME="${ANDROID_SDK_ROOT}/ndk/26.1.10909125"

# Rust toolchain + Android targets (`android/build.sh` installs cargo-ndk if missing).
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable \
    && rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android

WORKDIR /src
COPY . .

RUN chmod +x android/gradlew android/build.sh \
    && ./android/build.sh
