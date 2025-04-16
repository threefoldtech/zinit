# Installing Zinit

This guide provides detailed instructions for installing Zinit on various platforms.

## System Requirements

Zinit has minimal system requirements:

- Linux-based operating system
- Root access (for running as init system)

## Pre-built Binaries

If pre-built binaries are available for your system, you can install them directly:

```bash
# Download the binary (replace with actual URL)
wget https://github.com/threefoldtech/zinit/releases/download/vX.Y.Z/zinit-x86_64-unknown-linux-musl

# Make it executable
chmod +x zinit-x86_64-unknown-linux-musl

# Move to a location in your PATH
sudo mv zinit-x86_64-unknown-linux-musl /usr/local/bin/zinit
```

## Building from Source

### Prerequisites

To build Zinit from source, you'll need:

- Rust toolchain (1.46.0 or later recommended)
- musl and musl-tools packages
- GNU Make

#### Install Rust

If you don't have Rust installed, use rustup:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

#### Install musl development tools

On Debian/Ubuntu:

```bash
sudo apt update
sudo apt install musl musl-tools
```

On Fedora:

```bash
sudo dnf install musl musl-devel
```

On Alpine Linux (musl is already the default libc):

```bash
apk add build-base
```

### Build Process

1. Clone the repository:

```bash
git clone https://github.com/threefoldtech/zinit.git
cd zinit
```

2. Build using make:

```bash
make
```

This will create a statically linked binary at `target/x86_64-unknown-linux-musl/release/zinit`.

3. Install the binary:

```bash
sudo cp target/x86_64-unknown-linux-musl/release/zinit /usr/local/bin/
```

### Development Build

For development or debugging:

```bash
make dev
```

## Docker Installation

### Using the Provided Dockerfile

Zinit includes a test Docker image:

```bash
# Build the Docker image
make docker

# Run the container
docker run -dt --device=/dev/kmsg:/dev/kmsg:rw zinit
```

### Custom Docker Setup

To create your own Dockerfile with Zinit:

```dockerfile
FROM alpine:latest

# Install dependencies if needed
RUN apk add --no-cache bash curl

# Copy the zinit binary
COPY zinit /usr/local/bin/zinit
RUN chmod +x /usr/local/bin/zinit

# Create configuration directory
RUN mkdir -p /etc/zinit

# Add your service configurations
COPY services/*.yaml /etc/zinit/

# Set zinit as the entrypoint
ENTRYPOINT ["/usr/local/bin/zinit", "init", "--container"]
```

## Using Zinit as the Init System

To use Zinit as the init system (PID 1) on a Linux system:

### On a Standard Linux System

1. Install Zinit as described above
2. Create your service configurations in `/etc/zinit/`
3. Configure your bootloader to use zinit as init

For GRUB, add `init=/usr/local/bin/zinit` to the kernel command line:

```bash
# Edit GRUB configuration
sudo nano /etc/default/grub

# Add init parameter to GRUB_CMDLINE_LINUX
# Example:
# GRUB_CMDLINE_LINUX="init=/usr/local/bin/zinit"

# Update GRUB
sudo update-grub
```

### In a Container Environment

For containers, simply set Zinit as the entrypoint:

```bash
docker run -dt --device=/dev/kmsg:/dev/kmsg:rw \
  --entrypoint /usr/local/bin/zinit \
  your-image init --container
```

## First-time Setup

After installation, you'll need to create a basic configuration:

1. Create the configuration directory:

```bash
sudo mkdir -p /etc/zinit
```

2. Create a simple service configuration:

```bash
cat << EOF | sudo tee /etc/zinit/hello.yaml
exec: "echo 'Hello from Zinit!'"
oneshot: true
EOF
```

3. Test Zinit without running as init:

```bash
# For testing only - doesn't replace system init
sudo zinit init
```

If all is working correctly, you should see Zinit start and run your service.

## Upgrading Zinit

To upgrade an existing Zinit installation:

1. Stop any running Zinit instances (if not running as init)
2. Download or build the new version
3. Replace the existing binary:

```bash
sudo cp new-zinit /usr/local/bin/zinit
```

4. Restart Zinit or reboot the system

## Troubleshooting Installation

### Common Issues

#### Permission Denied

If you get "permission denied" errors:

```bash
sudo chmod +x /usr/local/bin/zinit
```

#### Missing Libraries

If you encounter missing library errors when running the binary, ensure you built with musl for a statically linked binary:

```bash
# Rebuild with musl
make
```

#### Socket Connection Issues

If you can't connect to the Zinit socket:

```bash
# Verify the socket exists
ls -la /var/run/zinit.sock

# Check permissions
sudo chmod 755 /var/run/zinit.sock
```

## Next Steps

Once installed, refer to the [Quickstart Guide](quickstart.md) to begin using Zinit.