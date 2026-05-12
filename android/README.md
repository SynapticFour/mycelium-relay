# Android Build & Test Guide

This guide explains how to build and test the Android app with embedded Rust (`mycelium-ffi`) and UniFFI bindings.

## Prerequisites

- Android Studio (latest stable)
- Android SDK + NDK installed
- Rust toolchain
- Java 17
- `cargo-ndk` (the script installs it automatically if missing)
- **Gradle wrapper** (`android/gradlew`, …) is checked in so you do not need a global Gradle install. To regenerate the wrapper with Docker only:

```bash
make gradle-wrapper
```

## 1) Generate Rust libs + Kotlin bindings + APK

From workspace root:

```bash
./android/build.sh
```

This does:

1. Adds Android Rust targets
2. Builds `mycelium-ffi` `.so` libraries with `cargo ndk`
3. Generates Kotlin bindings from `crates/mycelium-ffi/src/mycelium.udl`
4. Runs Android Gradle build (`./android/gradlew assembleDebug`)

## 1b) Full build inside Docker (no local Android SDK / NDK)

From the **workspace root** (needs Docker, long first run for SDK + Gradle + Rust caches):

```bash
make docker-android-ci
# or:
make docker-compose-android
```

This builds the image defined in `docker/android-ci/Dockerfile` (see also `docker-compose.yml` profile `android`), which installs JDK 17, Android SDK/NDK, Rust + `cargo-ndk`, then runs `./android/build.sh` so the APK is produced entirely in the container. The image targets **linux/amd64** by default (see `FROM` in the Dockerfile), which avoids some host-triple issues on arm64 Docker hosts; on Apple Silicon the first build can be slower due to emulation.

## 1c) Manual GitHub Action → Google Play

Workflow file: `.github/workflows/android-release-play.yml` (workflow dispatch only).

Configure the repository secrets listed in the header of that workflow, then run **Actions → Android Release → Google Play → Run workflow**. The job cross-compiles **arm64-v8a**, **armeabi-v7a**, and **x86_64**, builds a signed **AAB**, and uploads to the chosen Play track together with R8 **mapping.txt** and **whatsnew** (en-US).

## 2) Manual commands (optional)

Generate Kotlin bindings only:

```bash
cargo run -p mycelium-ffi --bin uniffi-bindgen -- generate \
  crates/mycelium-ffi/src/mycelium.udl \
  --language kotlin \
  --out-dir android/uniffi-bindings/src/main/kotlin/
```

Build Rust Android libs only:

```bash
cargo ndk \
  -t aarch64-linux-android \
  -t x86_64-linux-android \
  -o android/rust-build \
  build --release -p mycelium-ffi
```

Build debug APK only:

```bash
cd android
./gradlew assembleDebug
```

## 3) Run on emulator/device

1. Start an emulator (API 26+), or connect a device.
2. Install app:

```bash
cd android
./gradlew installDebug
```

3. Launch app. `MeshService` starts as foreground service and initializes the Rust node.

## 4) Basic E2E smoke test (2 emulators)

1. Start AVD A and AVD B.
2. Launch app on both.
3. In each app, open Peers screen.
4. Use QR flow or manual bootstrap add:
   - Scan QR from other device, or
   - Paste `/ip4/<ip>/tcp/7761/p2p/<peer_id>` and tap `Add`.
5. Verify peer appears in list.
6. Send chat message from A to B and verify receipt notification.

## 5) Android tests

Unit tests (local JVM):

```bash
cd android
./gradlew testDebugUnitTest
```

Instrumentation tests (device/emulator):

```bash
cd android
./gradlew connectedDebugAndroidTest
```

## Troubleshooting

- **`gradlew` missing**: generate wrapper in Android Studio or run `gradle wrapper` in `android/`.
- **No `.so` loaded**: ensure `android/rust-build` contains target ABI folders and `libmycelium_ffi.so`.
- **Binding mismatch**: re-run UniFFI generation after UDL changes.
- **Camera/QR issues**: ensure camera permission granted on device.
- **Peer discovery on hotspot**: use QR/manual bootstrap multiaddr flow (`addBootstrapPeer`).
