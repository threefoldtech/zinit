#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

# GitHub repository information
GITHUB_REPO="threefoldtech/zinit"

# Get the latest version from GitHub API
echo -e "${YELLOW}Fetching latest version information...${NC}"
if command -v curl &> /dev/null; then
    VERSION=$(curl -s "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" | grep -o '"tag_name": "[^"]*' | grep -o '[^"]*$')
elif command -v wget &> /dev/null; then
    VERSION=$(wget -qO- "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" | grep -o '"tag_name": "[^"]*' | grep -o '[^"]*$')
else
    echo -e "${RED}Neither curl nor wget found. Please install one of them and try again.${NC}"
    exit 1
fi

if [ -z "$VERSION" ]; then
    echo -e "${RED}Failed to fetch the latest version. Please check your internet connection.${NC}"
    exit 1
fi

echo -e "${GREEN}Latest version: ${VERSION}${NC}"
DOWNLOAD_URL="https://github.com/${GITHUB_REPO}/releases/download/${VERSION}"
MIN_SIZE_BYTES=2000000 # 2MB in bytes

echo -e "${GREEN}Installing zinit ${VERSION}...${NC}"

# Create temporary directory
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

# Map architecture names
if [ "$ARCH" = "x86_64" ]; then
    ARCH_NAME="x86_64"
elif [ "$ARCH" = "aarch64" ] || [ "$ARCH" = "arm64" ]; then
    ARCH_NAME="aarch64"
else
    echo -e "${RED}Unsupported architecture: $ARCH${NC}"
    exit 1
fi

# Determine binary name based on OS and architecture
if [ "$OS" = "linux" ]; then
    if [ "$ARCH_NAME" = "x86_64" ]; then
        BINARY_NAME="zinit-linux-x86_64"
    else
        echo -e "${RED}Unsupported Linux architecture: $ARCH${NC}"
        exit 1
    fi
elif [ "$OS" = "darwin" ]; then
    if [ "$ARCH_NAME" = "x86_64" ]; then
        BINARY_NAME="zinit-macos-x86_64"
    elif [ "$ARCH_NAME" = "aarch64" ]; then
        BINARY_NAME="zinit-macos-aarch64"
    else
        echo -e "${RED}Unsupported macOS architecture: $ARCH${NC}"
        exit 1
    fi
else
    echo -e "${RED}Unsupported operating system: $OS${NC}"
    exit 1
fi

# Download URL
DOWNLOAD_PATH="${DOWNLOAD_URL}/${BINARY_NAME}"
LOCAL_PATH="${TMP_DIR}/${BINARY_NAME}"

echo -e "${YELLOW}Detected: $OS on $ARCH_NAME${NC}"
echo -e "${YELLOW}Downloading from: $DOWNLOAD_PATH${NC}"

# Download the binary
if command -v curl &> /dev/null; then
    curl -L -o "$LOCAL_PATH" "$DOWNLOAD_PATH"
elif command -v wget &> /dev/null; then
    wget -O "$LOCAL_PATH" "$DOWNLOAD_PATH"
else
    echo -e "${RED}Neither curl nor wget found. Please install one of them and try again.${NC}"
    exit 1
fi

# Check file size
FILE_SIZE=$(stat -f%z "$LOCAL_PATH" 2>/dev/null || stat -c%s "$LOCAL_PATH" 2>/dev/null)
if [ "$FILE_SIZE" -lt "$MIN_SIZE_BYTES" ]; then
    echo -e "${RED}Downloaded file is too small (${FILE_SIZE} bytes). Expected at least ${MIN_SIZE_BYTES} bytes.${NC}"
    echo -e "${RED}This might indicate a failed or incomplete download.${NC}"
    exit 1
fi

echo -e "${GREEN}Download successful. File size: $(echo "$FILE_SIZE / 1000000" | bc -l | xargs printf "%.2f") MB${NC}"

# Make the binary executable
chmod +x "$LOCAL_PATH"

# Determine installation directory
if [ "$OS" = "darwin" ]; then
    # macOS - install to ~/hero/bin/
    INSTALL_DIR="$HOME/hero/bin"
else
    # Linux - install to /usr/local/bin/ if running as root, otherwise to ~/.local/bin/
    if [ "$(id -u)" -eq 0 ]; then
        INSTALL_DIR="/usr/local/bin"
    else
        INSTALL_DIR="$HOME/.local/bin"
        # Ensure ~/.local/bin exists and is in PATH
        mkdir -p "$INSTALL_DIR"
        if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
            echo -e "${YELLOW}Adding $INSTALL_DIR to your PATH. You may need to restart your terminal.${NC}"
            if [ -f "$HOME/.bashrc" ]; then
                echo "export PATH=\"\$PATH:$INSTALL_DIR\"" >> "$HOME/.bashrc"
            fi
            if [ -f "$HOME/.zshrc" ]; then
                echo "export PATH=\"\$PATH:$INSTALL_DIR\"" >> "$HOME/.zshrc"
            fi
        fi
    fi
fi

# Create installation directory if it doesn't exist
mkdir -p "$INSTALL_DIR"

# Copy the binary to the installation directory
cp "$LOCAL_PATH" "$INSTALL_DIR/zinit"
echo -e "${GREEN}Installed zinit to $INSTALL_DIR/zinit${NC}"

# Test the installation
echo -e "${YELLOW}Testing installation...${NC}"
if "$INSTALL_DIR/zinit" --help &> /dev/null; then
    echo -e "${GREEN}Installation successful! You can now use 'zinit' command.${NC}"
    echo -e "${YELLOW}Example usage: zinit --help${NC}"
    "$INSTALL_DIR/zinit" --help | head -n 5
else
    echo -e "${RED}Installation test failed. Please check the error messages above.${NC}"
    exit 1
fi

echo -e "${GREEN}zinit ${VERSION} has been successfully installed!${NC}"