#!/bin/bash

# Jump to the directory of the script
cd "$(dirname "$0")"

# Function to kill zinit and its children
kill_zinit() {
    echo "Checking for running zinit processes..."
    
    # Find zinit processes
    ZINIT_PIDS=$(pgrep -f "zinit" 2>/dev/null)
    
    if [ -n "$ZINIT_PIDS" ]; then
        echo "Found zinit processes: $ZINIT_PIDS"
        
        # Kill children first, then parent processes
        for pid in $ZINIT_PIDS; do
            echo "Killing zinit process $pid and its children..."
            # Kill process tree (parent and children)
            pkill -TERM -P $pid 2>/dev/null
            kill -TERM $pid 2>/dev/null
        done
        
        # Wait a moment for graceful shutdown
        sleep 2
        
        # Force kill if still running
        REMAINING_PIDS=$(pgrep -f "zinit" 2>/dev/null)
        if [ -n "$REMAINING_PIDS" ]; then
            echo "Force killing remaining zinit processes..."
            for pid in $REMAINING_PIDS; do
                pkill -KILL -P $pid 2>/dev/null
                kill -KILL $pid 2>/dev/null
            done
        fi
        
        echo "Zinit processes terminated."
    else
        echo "No running zinit processes found."
    fi
}

# Kill any existing zinit processes
kill_zinit

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
