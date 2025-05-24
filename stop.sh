#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}Stopping zinit...${NC}"

# Function to check if zinit is running
is_zinit_running() {
    pgrep -f "zinit" > /dev/null
    return $?
}

# Try to shutdown zinit gracefully if it's running
if is_zinit_running; then
    echo -e "${YELLOW}Zinit is already running. Attempting graceful shutdown...${NC}"
    zinit shutdown || true
    
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
