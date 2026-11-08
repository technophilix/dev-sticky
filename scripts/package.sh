#!/usr/bin/env sh
# Builds dist/devsticky_<ver>_amd64.deb and dist/devsticky-<ver>.x86_64.rpm on this machine.
# For packages that install on older distros too, use the GitHub Actions workflow
# (.github/workflows/release.yml), which builds on Ubuntu 24.04 and Fedora 41.
set -eu
cd "$(dirname "$0")/.."
if ! command -v cargo >/dev/null 2>&1 && [ -f "$HOME/.cargo/env" ]; then
  . "$HOME/.cargo/env"
fi
command -v cargo-deb >/dev/null 2>&1 || cargo install cargo-deb --locked
command -v cargo-generate-rpm >/dev/null 2>&1 || cargo install cargo-generate-rpm --locked

cargo deb
cargo generate-rpm
mkdir -p dist
cp target/debian/*.deb target/generate-rpm/*.rpm dist/
echo "Packages:"
ls -1 dist/
