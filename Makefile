SHELL := /bin/zsh

.PHONY: test test-fast test-unit test-sim check clippy ci fmt docker-android-ci docker-compose-android gradle-wrapper

# One-shot: add Gradle wrapper under android/ using Docker (no local Gradle needed).
gradle-wrapper:
	docker run --rm -v "$(CURDIR):/workspace" -w /workspace/android gradle:8.7-jdk17 \
		gradle wrapper --gradle-version=8.7 --distribution-type=bin

# Full Rust NDK + UniFFI + assembleDebug inside Docker (needs Docker + network).
docker-android-ci:
	docker build -f docker/android-ci/Dockerfile -t mycelium-android-ci "$(CURDIR)"

# Same as docker-android-ci via Compose (profile "android").
docker-compose-android:
	docker compose --profile android build android-ci

check:
	CARGO_HOME="$(CURDIR)/.cargo-home" cargo check --workspace

test-unit:
	CARGO_HOME="$(CURDIR)/.cargo-home" cargo test --workspace --lib

test-sim:
	CARGO_HOME="$(CURDIR)/.cargo-home" cargo test -p mycelium-sim

test:
	CARGO_HOME="$(CURDIR)/.cargo-home" cargo test --workspace

test-fast:
	CARGO_HOME="$(CURDIR)/.cargo-home" cargo test --workspace --lib

clippy:
	CARGO_HOME="$(CURDIR)/.cargo-home" cargo clippy --workspace --all-targets -- -D warnings

ci: check test-fast clippy
	CARGO_HOME="$(CURDIR)/.cargo-home" cargo fmt --all --check

fmt:
	CARGO_HOME="$(CURDIR)/.cargo-home" cargo fmt --all
