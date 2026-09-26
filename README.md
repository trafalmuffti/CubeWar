# CubeWar

A small first-person space shooter demo written in Rust with [Bevy](https://bevyengine.org).

You sit in the cockpit of a starfighter and shoot at Borg-inspired cube ships.
Only one cube exists at a time and it never shoots back. Each cube takes four hits:

| Hit | Effect |
|-----|--------|
| 1   | Shields come up, the cube glows **blue** |
| 2   | Shields strain, the glow turns **violet** |
| 3   | Shields collapse, the glow disappears |
| 4   | The cube is destroyed and replaced by a brief explosion; a new cube spawns off screen and drifts into view |

## Controls

| Input | Action |
|-------|--------|
| Mouse | Aim |
| Left mouse button / Space | Fire twin lasers |
| Esc | Release the mouse cursor (click to grab it again) |

## Building on Linux (native)

```sh
sudo apt install libasound2-dev libudev-dev   # Debian/Ubuntu
cargo run --release
```

## Building the Windows executable from Linux

The project cross-compiles to `x86_64-pc-windows-gnu` with MinGW. The linker is
configured in `.cargo/config.toml`.

```sh
sudo apt install gcc-mingw-w64-x86-64          # Debian/Ubuntu
rustup target add x86_64-pc-windows-gnu
./build-windows.sh                              # produces dist/cubewar.exe
```

Or by hand:

```sh
cargo build --release --target x86_64-pc-windows-gnu
# -> target/x86_64-pc-windows-gnu/release/cubewar.exe
```

The executable is statically linked against the MinGW runtime, so it runs on a
stock Windows 10/11 machine with no extra DLLs. A GitHub Actions workflow in
`.github/workflows/build.yml` performs the same cross-build and uploads the
`.exe` as an artifact.

## Debug / automation

Two environment variables exist for testing without a human at the controls:

| Variable | Effect |
|----------|--------|
| `CUBEWAR_AUTOPILOT=1` | The ship aims itself at the cube and fires every 1.5 s |
| `CUBEWAR_SCREENSHOT_DIR=<dir>` | Saves `frame_NNN.png` screenshots to `<dir>`; `CUBEWAR_SCREENSHOT_EVERY` sets the interval in seconds (default 2) |

Example headless smoke test on Linux with Mesa's software Vulkan driver:

```sh
mkdir -p shots
CUBEWAR_AUTOPILOT=1 CUBEWAR_SCREENSHOT_DIR=shots timeout 60 xvfb-run cargo run
```

## Requirements

Rust 1.95 or newer (Bevy 0.19).
