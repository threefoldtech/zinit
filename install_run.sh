#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}Starting zinit installation and setup...${NC}"
# Download and execute install.sh
echo -e "${YELLOW}Downloading and executing install.sh...${NC}"
curl -fsSL https://raw.githubusercontent.com/threefoldtech/zinit/refs/heads/master/install.sh | bash

# Determine the path to zinit based on OS
if [ "$(uname -s)" = "Darwin" ]; then
    ZINIT_PATH="$HOME/hero/bin/zinit"
else
    if [ "$(id -u)" -eq 0 ]; then
        ZINIT_PATH="/usr/local/bin/zinit"
    else
        ZINIT_PATH="$HOME/.local/bin/zinit"
    fi
fi

# Ensure zinit is in PATH
export PATH="$PATH:$(dirname "$ZINIT_PATH")"

# Function to check if zinit is running
is_zinit_running() {
    pgrep -f "zinit$" > /dev/null
    return $?
}

# Try to shutdown zinit gracefully if it's running
if is_zinit_running; then
    echo -e "${YELLOW}Zinit is already running. Attempting graceful shutdown...${NC}"
    "$ZINIT_PATH" shutdown || true
    
    # Give it a moment to shut down
    sleep 2
    
    # Check if it's still running
    if is_zinit_running; then
        echo -e "${YELLOW}Zinit is still running. Attempting to kill the process...${NC}"
        pkill -f "zinit$" || true
        sleep 1
    fi
else
    echo -e "${YELLOW}No existing zinit process found.${NC}"
fi

# Double-check no zinit is running
if is_zinit_running; then
    echo -e "${RED}Warning: Could not terminate existing zinit process. You may need to manually kill it.${NC}"
    ps aux | grep "zinit" | grep -v grep
else
    echo -e "${GREEN}No zinit process is running. Ready to start a new instance.${NC}"
fi

# Launch zinit in the background
echo -e "${GREEN}Starting zinit in the background...${NC}"
"$ZINIT_PATH" &

# Give it a moment to start
sleep 1

# Verify zinit is running
if is_zinit_running; then
    echo -e "${GREEN}Zinit is now running in the background.${NC}"
    echo -e "${YELLOW}You can manage services with:${NC}"
    echo -e "  ${YELLOW}$ZINIT_PATH list${NC}        - List all services"
    echo -e "  ${YELLOW}$ZINIT_PATH status${NC}      - Show status of all services"
    echo -e "  ${YELLOW}$ZINIT_PATH monitor${NC}     - Monitor services in real-time"
    echo -e "  ${YELLOW}$ZINIT_PATH shutdown${NC}    - Shutdown zinit when needed"
else
    echo -e "${RED}Failed to start zinit. Please check for errors above.${NC}"
    exit 1
fi

echo -e "${GREEN}Zinit installation and startup complete!${NC}"