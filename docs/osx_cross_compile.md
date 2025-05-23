# macOS Guide for Zinit

This guide covers both building Zinit natively on macOS and cross-compiling from macOS to Linux targets.

## Building Zinit Natively on macOS

Zinit can now be built and run directly on macOS. The code has been updated to handle platform-specific differences between Linux and macOS.

### Building for macOS

```bash
# Build a release version for macOS
make release-macos

# Install to ~/hero/bin (if it exists)
make install-macos
```

The native macOS build provides most of Zinit's functionality, with the following limitations:
- System reboot and shutdown operations are not supported (they will exit the process instead)
- Some Linux-specific features are disabled

## Cross-Compilation from macOS to Linux

This section outlines the steps to set up your macOS environment for cross-compiling Rust projects to the `aarch64-unknown-linux-musl` target. This is particularly useful for building binaries that can run on ARM-based Linux systems (e.g., Raspberry Pi, AWS Graviton) using musl libc.

## Prerequisites

*   Homebrew (https://brew.sh/) installed on your macOS system.
*   Rust and Cargo installed (e.g., via `rustup`).

## Step 1: Install the `aarch64-linux-musl-gcc` Toolchain

The `aarch64-linux-musl-gcc` toolchain is required for linking when cross-compiling to `aarch64-unknown-linux-musl`. You can install it using Homebrew:

```bash
brew install messense/macos-cross-toolchains/aarch64-linux-musl-cross
```

## Step 2: Link `musl-gcc`

Some build scripts or tools might look for `musl-gcc`. To ensure compatibility, create a symbolic link:

```bash
sudo ln -s /opt/homebrew/bin/aarch64-linux-musl-gcc /opt/homebrew/bin/musl-gcc
```

You might be prompted for your system password to complete this operation.

## Step 3: Add the Rust Target

Add the `aarch64-unknown-linux-musl` target to your Rust toolchain:

```bash
rustup target add aarch64-unknown-linux-musl
```

## Step 4: Build Your Project

Now you can build your Rust project for the `aarch64-unknown-linux-musl` target using Cargo:

```bash
cargo build --release --target aarch64-unknown-linux-musl
```

Alternatively, if you are using the provided `Makefile`, you can use the new target:

```bash
make release-aarch64-musl
```

This will produce a release binary located in `target/aarch64-unknown-linux-musl/release/`.

## Step 5: copy to osx hero bin

```bash
cp target/aarch64-unknown-linux-musl/release/zinit ~/hero/bin
```