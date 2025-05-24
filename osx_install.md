# macOS Installation Guide for Cross-Compilation to aarch64-unknown-linux-musl

This guide outlines the steps to set up your macOS environment for cross-compiling Rust projects to the `aarch64-unknown-linux-musl` target. This is particularly useful for building binaries that can run on ARM-based Linux systems (e.g., Raspberry Pi, AWS Graviton) using musl libc.

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

## Step 5: Running Zinit

After building, you can run `zinit`. First, you need to start the `zinit` daemon using the `init` subcommand. On macOS, `zinit` will now default to using `~/hero/cfg/zinit` as its configuration directory. This will create the Unix socket at `/tmp/zinit.sock` and start managing services.

Once the directory is created (if it doesn't exist, `zinit` will create it automatically), you can run the `init` command:

```bash
/path/to/your/zinit init
```

Replace `/path/to/your/zinit` with the actual path to your compiled `zinit` binary (e.g., `~/hero/bin/zinit` if you copied it there).

Once the daemon is running, you can interact with it using other `zinit` commands. For example, to list services:

```bash
/path/to/your/zinit list
```

If you encounter a "failed to connect to socket" error, ensure that the `zinit init` command is running in the background.
