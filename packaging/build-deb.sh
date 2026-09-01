#!/usr/bin/env bash
# Build a Debian package for ts570d-radio-control.
# Usage: ./packaging/build-deb.sh [--skip-build]
#
# Outputs: ts570d-radio-control_<version>_amd64.deb in the project root.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

VERSION="$(grep '^version' "${ROOT}/Cargo.toml" | head -1 | sed 's/.*= *"\(.*\)"/\1/')"
ARCH="amd64"
PKG="ts570d-radio-control_${VERSION}_${ARCH}"
STAGING="${ROOT}/target/debian/${PKG}"

# ── 1. Build release binaries ────────────────────────────────────────────────
if [[ "${1:-}" != "--skip-build" ]]; then
    # --workspace is load-bearing. The root Cargo.toml is a workspace root
    # that is ALSO a package, and in that layout a bare `cargo build`
    # builds only the root package and its dependencies. `emulator` and
    # `pin-test` are members nothing at the root depends on, so they were
    # never rebuilt -- the staging step below then installed whatever
    # happened to be left in target/release from some earlier build.
    #
    # That shipped silently: the 0.3.0 package built this way carried an
    # emulator eight days old, from before it had a network interface at
    # all. A stale binary in a package is worse than a build failure,
    # because nothing anywhere says so.
    echo "==> cargo build --release --workspace"
    (cd "${ROOT}" && cargo build --release --workspace)

    # `pin-test` is not in this workspace at all any more -- it moved to
    # radio-cat-rs's cat-transport-serial as a shared [[bin]] (CLAUDE.md).
    # Without this line the staging step below installed an orphan: a
    # binary left in target/release by a build predating the move, from
    # source this repo no longer contains. On a clean checkout it would
    # instead fail here with a missing file, which is at least loud.
    echo "==> cargo build --release -p cat-transport-serial --bin pin-test"
    (cd "${ROOT}" && cargo build --release -p cat-transport-serial --bin pin-test)
fi

RELEASE="${ROOT}/target/release"

# ── 2. Stage package tree ────────────────────────────────────────────────────
echo "==> Staging into ${STAGING}"
rm -rf "${STAGING}"
install -d "${STAGING}/DEBIAN"
install -d "${STAGING}/usr/bin"
install -d "${STAGING}/usr/share/doc/ts570d-radio-control"
install -d "${STAGING}/usr/share/man/man1"

# Binaries — rename to final installed names
install -m 0755 "${RELEASE}/ts570d"      "${STAGING}/usr/bin/ts570d-control"
install -m 0755 "${RELEASE}/emulator"    "${STAGING}/usr/bin/ts570d-emulator"
install -m 0755 "${RELEASE}/pin-test"    "${STAGING}/usr/bin/rs232c-pintest"
install -m 0755 "${RELEASE}/ts570d-line" "${STAGING}/usr/bin/ts570d-line"
install -m 0755 "${RELEASE}/ts570d-gui"  "${STAGING}/usr/bin/ts570d-gui"

# Refuse to package anything older than this build. The failure that
# motivates this shipped an eight-day-old emulator inside a package
# labelled with today's version, and nothing anywhere said so -- a stale
# binary is worse than a build failure precisely because it is quiet.
NEWEST_SOURCE="$(find "${ROOT}" -name '*.rs' -newer "${STAGING}/usr/bin/ts570d-control" -not -path '*/target/*' -print -quit)"
if [[ -n "${NEWEST_SOURCE}" ]]; then
    echo "ERROR: ${NEWEST_SOURCE} is newer than the binaries being packaged." >&2
    echo "       Re-run without --skip-build." >&2
    exit 1
fi

# Control file (substitute version)
sed "s/^Version:.*/Version: ${VERSION}/" \
    "${SCRIPT_DIR}/DEBIAN/control" > "${STAGING}/DEBIAN/control"

# Copyright — DEP-5 format with full Apache 2.0 license text from LICENSE.txt
{
    cat <<HEADER
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: ts570d-radio-control
Upstream-Contact: Matt Franklin <radiombf@gmail.com>
Source: https://github.com/kf0uwv/ts570d

Files: *
Copyright: 2026 Matt Franklin <radiombf@gmail.com>
License: Apache-2.0

License: Apache-2.0
HEADER
    # Indent every line of the license text by one space (DEP-5 requirement).
    # Blank lines become a single " ." to preserve paragraph breaks.
    sed 's/^$/ ./; s/^/ /' "${ROOT}/LICENSE.txt"
} > "${STAGING}/usr/share/doc/ts570d-radio-control/copyright"

# ── 3. Build .deb ────────────────────────────────────────────────────────────
OUT="${ROOT}/${PKG}.deb"
echo "==> dpkg-deb --build ${STAGING} ${OUT}"
dpkg-deb --build "${STAGING}" "${OUT}"

echo ""
echo "Package built: ${OUT}"
echo ""
dpkg-deb --info "${OUT}"
echo ""
echo "Contents:"
dpkg-deb --contents "${OUT}"
