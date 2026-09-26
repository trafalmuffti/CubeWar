#!/usr/bin/env bash
# Build the Windows executable from a Linux host.
set -euo pipefail
cd "$(dirname "$0")"

TARGET=x86_64-pc-windows-gnu

if ! command -v x86_64-w64-mingw32-gcc >/dev/null; then
  echo "Missing MinGW cross compiler. On Debian/Ubuntu: sudo apt install gcc-mingw-w64-x86-64" >&2
  exit 1
fi
rustup target add "$TARGET"

cargo build --release --target "$TARGET"

mkdir -p dist
cp "target/$TARGET/release/cubewar.exe" dist/
echo "Built dist/cubewar.exe"
