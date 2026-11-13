#!/bin/sh
# Does what .github/workflows/check.yml does, on this machine, for when
# GitHub Actions can't run: the interface build, clippy and the tests, in the
# same clean archlinux:base-devel container, on a copy of the working tree
# (committed or not, minus ignored files). Nothing in this checkout changes:
# no node_modules, target or dist are written here.
#
#   scripts/check-local.sh
#
# Needs podman (or docker).
set -eu

cd "$(dirname "$0")/../.."
engine=$(command -v podman || command -v docker) || {
    echo "check-local: needs podman or docker" >&2
    exit 1
}

git ls-files -z --cached --others --exclude-standard |
    tar --null -T - -cf - |
    "$engine" run --rm -i docker.io/library/archlinux:base-devel bash -ec '
        pacman -Syu --noconfirm --needed git rust
        mkdir /src && tar -xf - -C /src
        cd /src/quantum-linux
        source packaging/PKGBUILD
        pacman -S --noconfirm --needed "${depends[@]}" "${makedepends[@]}"
        echo "== Interface (typecheck and build)"
        npm --prefix ui ci --no-audit --no-fund
        npm --prefix ui run build
        echo "== Clippy"
        cargo clippy --locked --workspace --all-targets -- -D warnings
        echo "== Tests"
        cargo test --locked --workspace
        echo "== All checks passed"
    '
