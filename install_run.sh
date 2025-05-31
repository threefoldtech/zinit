#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

# Function to check if zinit is running
is_zinit_running() {
    if zinit list &>/dev/null; then
        return 0  # Command successful, zinit is running
    else
        return 1  # Command failed, zinit is not running
    fi
}

echo -e "${GREEN}Starting zinit installation and setup...${NC}"
# Download and execute install.sh
echo -e "${YELLOW}Downloading and executing install.sh...${NC}"
curl -fsSL https://raw.githubusercontent.com/threefoldtech/zinit/refs/heads/master/install.sh | bash

echo -e "${GREEN}install zinit...${NC}"
rm -f /tmp/install.sh
curl -fsSL https://raw.githubusercontent.com/threefoldtech/zinit/refs/heads/master/install.sh > /tmp/install.sh
bash /tmp/install.sh


# Launch zinit in the background
echo -e "${GREEN}Starting zinit in the background...${NC}"
zinit &

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