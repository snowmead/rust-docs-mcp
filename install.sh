#!/bin/bash
set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
REPO_URL="https://github.com/snowmead/rust-docs-mcp.git"
TEMP_DIR=""
INSTALL_DIR="${HOME}/.local/bin"

# Cleanup function
cleanup() {
    if [[ -n "${TEMP_DIR}" && -d "${TEMP_DIR}" ]]; then
        echo -e "${BLUE}Cleaning up temporary files...${NC}"
        rm -rf "${TEMP_DIR}"
    fi
}
trap cleanup EXIT

# Helper functions
info() { echo -e "${BLUE}[INFO]${NC} $1"; }
success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# Check if command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Install Rust if not present
install_rust() {
    if command_exists rustc && command_exists cargo; then
        info "Rust is already installed ($(rustc --version))"
        
        # Check if nightly toolchain is available
        if rustup toolchain list | grep -q nightly; then
            info "Rust nightly toolchain is already available"
        else
            info "Installing Rust nightly toolchain..."
            if ! rustup toolchain install nightly; then
                error "Failed to install Rust nightly toolchain"
            fi
            success "Rust nightly toolchain installed"
        fi
        return 0
    fi
    
    info "Installing Rust..."
    if ! curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable; then
        error "Failed to install Rust"
    fi
    
    # Source the environment
    if [[ -f "${HOME}/.cargo/env" ]]; then
        # shellcheck source=/dev/null
        source "${HOME}/.cargo/env"
    fi
    
    if ! command_exists cargo; then
        error "Rust installation failed - cargo not found in PATH"
    fi
    
    success "Rust installed successfully"
    
    # Install nightly toolchain
    info "Installing Rust nightly toolchain..."
    if ! rustup toolchain install nightly; then
        error "Failed to install Rust nightly toolchain"
    fi
    success "Rust nightly toolchain installed"
}

# Main installation function
main() {
    echo -e "${GREEN}🦀 rust-docs-mcp Installer${NC}"
    echo "========================================="
    
    # Check for required tools
    if ! command_exists git; then
        error "git is required but not installed"
    fi
    
    if ! command_exists curl; then
        error "curl is required but not installed"
    fi
    
    # Install Rust if needed
    install_rust
    
    # Create temporary directory
    TEMP_DIR=$(mktemp -d)
    info "Using temporary directory: ${TEMP_DIR}"
    
    # Clone repository
    info "Cloning rust-docs-mcp repository..."
    if ! git clone --depth 1 "${REPO_URL}" "${TEMP_DIR}/rust-docs-mcp"; then
        error "Failed to clone repository"
    fi
    
    # Build and install
    info "Building rust-docs-mcp in release mode (this may take a few minutes)..."
    cd "${TEMP_DIR}/rust-docs-mcp"
    
    if ! cargo build --release -p rust-docs-mcp; then
        error "Failed to build rust-docs-mcp"
    fi
    
    # Install using the built-in install command
    info "Installing rust-docs-mcp to ${INSTALL_DIR}..."
    if ! "${TEMP_DIR}/rust-docs-mcp/target/release/rust-docs-mcp" install --target-dir "${INSTALL_DIR}" --force; then
        error "Failed to install rust-docs-mcp"
    fi
    
    # Handle macOS code signing to prevent Gatekeeper from killing the binary
    if [[ "$OSTYPE" == "darwin"* ]]; then
        info "Signing binary for macOS..."
        # Remove any quarantine attributes
        xattr -cr "${INSTALL_DIR}/rust-docs-mcp" 2>/dev/null || true
        # Ad-hoc sign the binary
        if codesign --force --deep -s - "${INSTALL_DIR}/rust-docs-mcp" 2>/dev/null; then
            success "Binary signed successfully"
        else
            warn "Could not sign binary - you may need to run: codesign --force --deep -s - ${INSTALL_DIR}/rust-docs-mcp"
        fi
    fi
    
    success "rust-docs-mcp installed successfully!"
    
    # Check if install directory is in PATH
    if [[ ":${PATH}:" != *":${INSTALL_DIR}:"* ]]; then
        warn "${INSTALL_DIR} is not in your PATH"
        echo
        echo -e "${YELLOW}Add this line to your shell configuration file (.bashrc, .zshrc, etc.):${NC}"
        echo -e "${BLUE}export PATH=\"${INSTALL_DIR}:\$PATH\"${NC}"
        echo
        echo -e "${YELLOW}Then reload your shell or run:${NC}"
        echo -e "${BLUE}source ~/.bashrc  # or ~/.zshrc${NC}"
    else
        echo
        echo -e "${GREEN}✅ You can now run 'rust-docs-mcp' from anywhere!${NC}"
    fi
    
    # Check if Claude Code is installed and ask about MCP server setup
    if command_exists claude && claude --version >/dev/null 2>&1; then
        echo
        echo -e "${BLUE}Would you like to add rust-docs-mcp to Claude Code as an MCP server?${NC}"
        echo -e "${YELLOW}This will enable you to use Rust documentation features directly in Claude Code.${NC}"
        echo
        
        # Try to read user input - handle both direct execution and piped execution
        REPLY="n"
        
        # Check if script is being piped
        if [ ! -t 0 ]; then
            # We're being piped - try to open /dev/tty directly
            if [ -e /dev/tty ]; then
                # Use a subshell to read from /dev/tty
                REPLY=$(bash -c 'read -p "Add to Claude Code? [y/N] " -n 1 -r REPLY < /dev/tty > /dev/tty 2>&1 && echo "$REPLY"') || REPLY="n"
                echo > /dev/tty
            else
                # No terminal available
                echo -e "${YELLOW}Running in non-interactive mode (piped execution detected).${NC}"
                echo -e "${YELLOW}To enable interactive prompts, download and run the installer directly:${NC}"
                echo
                echo -e "${BLUE}  curl -sSL ${REPO_URL%.git}/raw/main/install.sh -o install.sh${NC}"
                echo -e "${BLUE}  bash install.sh${NC}"
                echo
                REPLY="n"
            fi
        else
            # Direct execution - normal read should work
            read -p "Add to Claude Code? [y/N] " -n 1 -r REPLY || REPLY="n"
            echo
        fi
        
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            info "Adding rust-docs-mcp to Claude Code..."
            
            # Add the MCP server to Claude Code
            if claude mcp add rust-docs -s user "${INSTALL_DIR}/rust-docs-mcp" -t stdio; then
                success "rust-docs-mcp added to Claude Code!"
                echo
                echo -e "${GREEN}You can now use Rust documentation features in Claude Code!${NC}"
                echo
                echo -e "${YELLOW}To enable all rust-docs tools, add these to your Claude settings allow list:${NC}"
                echo -e "${BLUE}(~/.claude/settings.json or your project/local settings)${NC}"
                echo
                echo -e "${GREEN}
      \"mcp__rust-docs__cache_crate_from_cratesio\",
      \"mcp__rust-docs__cache_crate_from_github\",
      \"mcp__rust-docs__cache_crate_from_local\",
      \"mcp__rust-docs__remove_crate\",
      \"mcp__rust-docs__list_cached_crates\",
      \"mcp__rust-docs__list_crate_versions\",
      \"mcp__rust-docs__get_crates_metadata\",
      \"mcp__rust-docs__list_crate_items\",
      \"mcp__rust-docs__search_items\",
      \"mcp__rust-docs__search_items_preview\",
      \"mcp__rust-docs__search_items_fuzzy\",
      \"mcp__rust-docs__get_item_details\",
      \"mcp__rust-docs__get_item_docs\",
      \"mcp__rust-docs__get_item_source\",
      \"mcp__rust-docs__get_dependencies\",
      \"mcp__rust-docs__structure\"${NC}"
            else
                warn "Failed to add rust-docs-mcp to Claude Code"
                echo
                echo -e "${YELLOW}You can try adding it manually with:${NC}"
                echo -e "${BLUE}claude mcp add rust-docs -s user ${INSTALL_DIR}/rust-docs-mcp -t stdio${NC}"
            fi
        else
            echo
            echo -e "${YELLOW}You can add rust-docs-mcp to Claude Code later with:${NC}"
            echo -e "${BLUE}claude mcp add rust-docs -s user ${INSTALL_DIR}/rust-docs-mcp -t stdio${NC}"
        fi
    fi
    
    echo
    echo -e "${BLUE}Usage:${NC}"
    echo -e "  ${GREEN}rust-docs-mcp${NC}                # Start MCP server"
    echo -e "  ${GREEN}rust-docs-mcp install${NC}        # Install/update to PATH"
    echo -e "  ${GREEN}rust-docs-mcp --help${NC}         # Show help"
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --install-dir)
            INSTALL_DIR="$2"
            shift 2
            ;;
        --help|-h)
            echo "rust-docs-mcp installer"
            echo
            echo "Usage: $0 [OPTIONS]"
            echo
            echo "Options:"
            echo "  --install-dir DIR    Install directory (default: ~/.local/bin)"
            echo "  --help, -h           Show this help"
            echo
            echo "Example:"
            echo "  curl -sSL https://raw.githubusercontent.com/snowmead/rust-docs-mcp/main/install.sh | bash"
            echo "  curl -sSL https://raw.githubusercontent.com/snowmead/rust-docs-mcp/main/install.sh | bash -s -- --install-dir /usr/local/bin"
            exit 0
            ;;
        *)
            error "Unknown option: $1"
            ;;
    esac
done

# Ensure install directory exists
mkdir -p "${INSTALL_DIR}"

# Run main installation
main