.PHONY: all build release test test-unit test-integration \
        clippy fmt fmt-check check clean deb emulator run help \
        windows-check windows-package

# ── Default ──────────────────────────────────────────────────────────────────
all: build

# ── Build ────────────────────────────────────────────────────────────────────
build:
	cargo build

release:
	cargo build --release
	cargo build --release -p emulator
	# pin-test is a shared cat-transport-serial `[[bin]]` in radio-cat-rs now
	# (see docs/adr/0006-windows-concurrency-model.md), not a local binary --
	# `-p` selects it by package ID across the resolved dependency graph.
	cargo build --release -p cat-transport-serial --bin pin-test

# ── Test ─────────────────────────────────────────────────────────────────────
test: test-unit test-integration

test-unit:
	cargo test --lib --all

test-integration:
	cargo test --test integration

# ── Lint / Format ────────────────────────────────────────────────────────────
clippy:
	cargo clippy --all-targets --all-features -- -D warnings

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

# fmt + clippy in one shot (matches CI)
check: fmt-check clippy

# ── Run ──────────────────────────────────────────────────────────────────────
# Usage: make run PORT=/dev/ttyS0
PORT ?= /dev/ttyS0
run:
	cargo run -- $(PORT)

emulator:
	cargo run --bin emulator

pintest:
	cargo run -p cat-transport-serial --bin pin-test

# ── Windows (local best-effort -- CI is the source of truth) ─────────────────
# Per `radio-cat-rs` ADR 0012, `x86_64-pc-windows-msvc` is the only Windows
# target; `x86_64-pc-windows-gnu` is retired. This cross-compiles to MSVC
# from Linux via cargo-xwin, which fetches the Microsoft CRT and Windows SDK
# (~1.1 GB, cached under ~/.cache/cargo-xwin). It is a fast local signal
# only: it cannot RUN the tests, and the authoritative check is the
# `windows-latest` CI job, which runs `cargo check` AND `cargo test`.
#
# One-time setup:
#   cargo install cargo-xwin --locked
#   rustup target add x86_64-pc-windows-msvc
windows-check:
	cargo xwin check --target x86_64-pc-windows-msvc --workspace --exclude emulator

# ── Package ──────────────────────────────────────────────────────────────────
deb: release
	./packaging/build-deb.sh --skip-build

# Windows packaging must run on/for a Windows target; this only stages the
# zip from already-cross-checked binaries built on a real Windows host (see
# packaging/build-windows-package.ps1 and CLAUDE.md). Not runnable in this
# repo's own Linux dev environment.
windows-package:
	pwsh ./packaging/build-windows-package.ps1

# ── Clean ────────────────────────────────────────────────────────────────────
clean:
	cargo clean
	rm -f ts570d-radio-control_*.deb
	rm -f ts570d-radio-control_*.zip

# ── Help ─────────────────────────────────────────────────────────────────────
help:
	@echo "Targets:"
	@echo "  build            Debug build (all crates)"
	@echo "  release          Release build (all binaries)"
	@echo "  test             Unit + integration tests"
	@echo "  test-unit        Unit tests only (--lib)"
	@echo "  test-integration Integration tests (requires PTY + io_uring)"
	@echo "  check            fmt-check + clippy (matches CI)"
	@echo "  fmt              Format all code"
	@echo "  clippy           Lint with -D warnings"
	@echo "  run [PORT=...]   Run control app (default: /dev/ttyS0)"
	@echo "  emulator         Run the virtual radio emulator"
	@echo "  pintest          Run RS-232C pin diagnostic"
	@echo "  deb              Build Debian package (.deb)"
	@echo "  windows-check    cargo xwin check for MSVC (local best-effort; CI is authoritative)"
	@echo "  windows-package  Build Windows .zip package (requires pwsh + Windows binaries)"
	@echo "  clean            Remove build artifacts and .deb/.zip files"
