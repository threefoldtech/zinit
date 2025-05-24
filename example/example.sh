#!/bin/bash

# Determine the zinit binary path
ZINIT_BIN="./target/release/zinit" # Assuming zinit is built in release mode in the current directory

# Determine the configuration directory based on OS
if [[ "$(uname)" == "Darwin" ]]; then
    # macOS
    ZINIT_CONFIG_DIR="$HOME/hero/cfg/zinit"
else
    # Linux or other
    ZINIT_CONFIG_DIR="/etc/zinit"
fi

SERVICE_NAME="test_service"
CPU_SERVICE_NAME="cpu_test_service"
SERVICE_FILE="$ZINIT_CONFIG_DIR/$SERVICE_NAME.yaml"
CPU_SERVICE_FILE="$ZINIT_CONFIG_DIR/$CPU_SERVICE_NAME.yaml"

echo "--- Zinit Example Script ---"
echo "Zinit binary path: $ZINIT_BIN"
echo "Zinit config directory: $ZINIT_CONFIG_DIR"

# Step 1: Ensure zinit config directory exists
echo "Ensuring zinit config directory exists..."
mkdir -p "$ZINIT_CONFIG_DIR"
if [ $? -ne 0 ]; then
    echo "Error: Failed to create config directory $ZINIT_CONFIG_DIR. Exiting."
    exit 1
fi
echo "Config directory $ZINIT_CONFIG_DIR is ready."

# Step 2: Check if zinit daemon is running, if not, start it in background
echo "Checking if zinit daemon is running..."
if "$ZINIT_BIN" list > /dev/null 2>&1; then
    echo "Zinit daemon is already running."
else
    echo "Zinit daemon not running. Starting it in background..."
    # Start zinit init in a new process group to avoid it being killed by script exit
    # and redirecting output to /dev/null
    nohup "$ZINIT_BIN" init > /dev/null 2>&1 &
    ZINIT_PID=$!
    echo "Zinit daemon started with PID: $ZINIT_PID"
    sleep 2 # Give zinit a moment to start up and create the socket
    if ! "$ZINIT_BIN" list > /dev/null 2>&1; then
        echo "Error: Zinit daemon failed to start. Exiting."
        exit 1
    fi
    echo "Zinit daemon successfully started."
fi

# Step 3: Create sample zinit service files
echo "Creating sample service file: $SERVICE_FILE"
cat <<EOF > "$SERVICE_FILE"
name: $SERVICE_NAME
exec: /bin/bash -c "while true; do echo 'Hello from $SERVICE_NAME!'; sleep 5; done"
log: stdout
EOF

if [ $? -ne 0 ]; then
    echo "Error: Failed to create service file $SERVICE_FILE. Exiting."
    exit 1
fi
echo "Service file created."

# Create a CPU-intensive service with child processes
echo "Creating CPU-intensive service file: $CPU_SERVICE_FILE"
cat <<EOF > "$CPU_SERVICE_FILE"
name: $CPU_SERVICE_NAME
exec: /bin/bash -c "for i in {1..3}; do (openssl speed -multi 2 &) ; done; while true; do sleep 10; done"
log: stdout
EOF

if [ $? -ne 0 ]; then
    echo "Error: Failed to create CPU service file $CPU_SERVICE_FILE. Exiting."
    exit 1
fi
echo "CPU service file created."

# Step 4: Tell zinit to monitor the new services
echo "Telling zinit to monitor the services..."
"$ZINIT_BIN" monitor "$SERVICE_NAME"
"$ZINIT_BIN" monitor "$CPU_SERVICE_NAME"

# Step 5: List services to verify the new service is recognized
echo "Listing zinit services to verify..."
"$ZINIT_BIN" list

# Step 6: Show stats for the CPU-intensive service
echo "Waiting for services to start and generate some stats..."
sleep 5
echo "Getting stats for $CPU_SERVICE_NAME..."
"$ZINIT_BIN" stats "$CPU_SERVICE_NAME"

# # Step 7: Clean up (optional, but good for examples)
# echo "Cleaning up: stopping and forgetting services..."
# "$ZINIT_BIN" stop "$SERVICE_NAME" > /dev/null 2>&1
# "$ZINIT_BIN" forget "$SERVICE_NAME" > /dev/null 2>&1
# "$ZINIT_BIN" stop "$CPU_SERVICE_NAME" > /dev/null 2>&1
# "$ZINIT_BIN" forget "$CPU_SERVICE_NAME" > /dev/null 2>&1
# rm -f "$SERVICE_FILE" "$CPU_SERVICE_FILE"
# echo "Cleanup complete."

echo "--- Script Finished ---"
