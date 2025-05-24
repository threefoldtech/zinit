#!/bin/bash

# Jump to the directory of the script
cd "$(dirname "$0")"

./stop.sh

# Build the project
echo "Building zinit..."
cargo build --release

if [ $? -ne 0 ]; then
    echo "Build failed!"
    exit 1
fi

# Copy the binary
echo "Copying zinit binary to ~/hero/bin..."
cp ./target/release/zinit ~/hero/bin

if [ $? -ne 0 ]; then
    echo "Failed to copy binary!"
    exit 1
fi

# Ensure config directory exists
echo "Ensuring config directory exists..."
mkdir -p ~/hero/cfg/zinit

# Start zinit in init mode (daemon) in background
echo "Starting zinit daemon in background..."
~/hero/bin/zinit init -c ~/hero/cfg/zinit &
ZINIT_PID=$!

# Wait a moment for zinit to start and create the socket
sleep 5

# Check if zinit is running
if kill -0 $ZINIT_PID 2>/dev/null; then
    echo "Zinit daemon started successfully with PID: $ZINIT_PID"
    
    # Test with zinit list
    echo "Testing zinit list command..."
    ~/hero/bin/zinit list
    
    if [ $? -eq 0 ]; then
        echo "Zinit is working correctly!"
    else
        echo "Warning: zinit list command failed, but zinit daemon is running"
        echo "This might be normal if no services are configured yet."
    fi
else
    echo "Failed to start zinit daemon!"
    exit 1
fi

echo "Build and setup completed successfully!"
