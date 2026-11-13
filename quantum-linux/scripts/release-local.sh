#!/bin/sh
# Does what .github/workflows/release.yml does, on this machine, for when
# GitHub Actions can't run: builds the Arch package for a pushed version tag
# in the same clean archlinux:base-devel container, then publishes the GitHub
# release with gh, which needs no Actions minutes.
#
# The container matters: a makepkg run on the host uses the host's
# makepkg.conf, and CachyOS's builds for this CPU only (-march=native,
# target-cpu=native), which would crash on older ones.
#
#   scripts/release-local.sh v0.5.1               build and publish
#   scripts/release-local.sh v0.5.1 --build-only  build, show the files, stop
#
# Needs podman (or docker) and a logged-in gh. Bump, commit, tag and push
# first: the package is built from the tag's source on GitHub.
set -eu

die() {
    echo "release-local: $*" >&2
    exit 1
}

tag=${1:-}
[ -n "$tag" ] || die "usage: $0 vX.Y.Z [--build-only]"
build_only=false
[ "${2:-}" = "--build-only" ] && build_only=true
version=${tag#v}

cd "$(dirname "$0")/.."
grep -qx "pkgver=$version" packaging/PKGBUILD || die "packaging/PKGBUILD pkgver is not $version"
grep -qx "version = \"$version\"" Cargo.toml || die "Cargo.toml version is not $version"
git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null || die "tag $tag isn't on GitHub yet; push it first"
if ! $build_only && gh release view "$tag" >/dev/null 2>&1; then
    die "release $tag already exists"
fi
engine=$(command -v podman || command -v docker) || die "needs podman or docker"

out=$(mktemp -d "${TMPDIR:-/tmp}/quantumengine-release.XXXXXX")
cp packaging/PKGBUILD "$out/"
echo "Building $tag in a clean archlinux:base-devel container; files go to $out"

# The same steps as the workflow's, as root in the container; makepkg itself
# refuses to run as root, so it runs as a user made for it.
"$engine" run --rm -v "$out:/out:Z" docker.io/library/archlinux:base-devel bash -ec '
    pacman -Syu --noconfirm --needed pacman-contrib
    source /out/PKGBUILD
    pacman -S --noconfirm --needed "${depends[@]}" "${makedepends[@]}"
    useradd --create-home builder
    install -d -o builder /home/builder/pkg
    install -o builder -m644 /out/PKGBUILD /home/builder/pkg/
    cd /home/builder/pkg
    sudo -u builder updpkgsums
    sudo -u builder makepkg --noconfirm
    for f in *.pkg.tar.zst; do sha256sum "$f" > "$f.sha256"; done
    cp ./*.pkg.tar.zst ./*.pkg.tar.zst.sha256 PKGBUILD /out/
'

ls -l "$out"
if $build_only; then
    echo "Built, not published. To publish these files: $0 $tag"
    exit 0
fi

gh release create "$tag" \
    --title "QuantumEngine $version" \
    --generate-notes \
    "$out"/*.pkg.tar.zst "$out"/*.pkg.tar.zst.sha256 "$out/PKGBUILD"
echo "Published $tag."
