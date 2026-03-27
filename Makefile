.PHONY: all build release test test-unit test-integration \
        clippy fmt fmt-check check clean deb emulator run help

# ── Default ──────────────────────────────────────────────────────────────────
all: build

# ── Build ────────────────────────────────────────────────────────────────────
build:
	cargo build

release:
	cargo build --release
	cargo build --release -p emulator
	cargo build --release -p serial

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
	cargo run -p serial --bin pin-test

# ── Package ──────────────────────────────────────────────────────────────────
deb: release
	./packaging/build-deb.sh --skip-build

# ── Clean ────────────────────────────────────────────────────────────────────
clean:
	cargo clean
	rm -f ts570d-radio-control_*.deb

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
	@echo "  clean            Remove build artifacts and .deb files"
