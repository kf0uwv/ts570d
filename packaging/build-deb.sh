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
    echo "==> cargo build --release"
    (cd "${ROOT}" && cargo build --release)
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
install -m 0755 "${RELEASE}/ts570d"    "${STAGING}/usr/bin/ts570d-control"
install -m 0755 "${RELEASE}/emulator"  "${STAGING}/usr/bin/ts570d-emulator"
install -m 0755 "${RELEASE}/pin-test"  "${STAGING}/usr/bin/rs232c-pintest"

# Control file (substitute version)
sed "s/^Version:.*/Version: ${VERSION}/" \
    "${SCRIPT_DIR}/DEBIAN/control" > "${STAGING}/DEBIAN/control"

# Copyright
cat > "${STAGING}/usr/share/doc/ts570d-radio-control/copyright" <<'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: ts570d-radio-control
Upstream-Contact: Matt Franklin <radiombf@gmail.com>
Source: https://github.com/kf0uwv/ts570d

Files: *
Copyright: 2024 Matt Franklin <radiombf@gmail.com>
License: MIT or Apache-2.0

License: MIT
 Permission is hereby granted, free of charge, to any person obtaining a copy
 of this software and associated documentation files (the "Software"), to deal
 in the Software without restriction, including without limitation the rights
 to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 copies of the Software, and to permit persons to whom the Software is
 furnished to do so, subject to the following conditions: [...]

License: Apache-2.0
 Licensed under the Apache License, Version 2.0 (the "License"); [...]
EOF

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
