# Installation Guide

This guide provides multiple ways to install `rust-docs-mcp` depending on your platform and preferences.

## Quick Start

### Bash Script Installation

The easiest way to install rust-docs-mcp is using our installation script:

```bash
curl -sSL https://raw.githubusercontent.com/snowmead/rust-docs-mcp/main/install.sh | bash
```

#### Custom Installation Directory

You can specify a custom installation directory:

```bash
curl -sSL https://raw.githubusercontent.com/snowmead/rust-docs-mcp/main/install.sh | bash -s -- --install-dir /usr/local/bin
```

The script will:
- Check for and install required dependencies (Rust, git)
- Install the Rust nightly toolchain if needed
- Build rust-docs-mcp in release mode
- Install the binary to `~/.local/bin` (or your specified directory)
- Handle macOS code signing automatically
- Offer to configure Claude Code integration if detected

## Package Manager Installation

> **Note**: Package manager installations currently build from the main branch as official releases are pending.

### Homebrew (macOS and Linux)

#### Using Official Formula
```bash
# Install directly (builds from source)
brew install snowmead/rust-docs-mcp

# Or add tap first, then install
brew tap snowmead
brew install rust-docs-mcp
```

#### Update
```bash
brew upgrade rust-docs-mcp
```

#### Uninstall
```bash
brew uninstall rust-docs-mcp
```

### Scoop (Windows)

#### Using Official Manifest
```powershell
# Install directly (builds from source)
scoop install https://raw.githubusercontent.com/snowmead/rust-docs-mcp/main/scoop/rust-docs-mcp.json

# Note: Bucket installation method is not yet available
```

#### Update
```powershell
scoop update rust-docs-mcp
```

#### Uninstall
```powershell
scoop uninstall rust-docs-mcp
```

## Manual Installation

### Prerequisites
- **Rust**: Version 1.70.0 or higher
- **Git**: For downloading the source code
- **Rust Nightly Toolchain**: Specifically `nightly-2025-06-23`

### Install Prerequisites

#### Install Rust
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

#### Install Required Nightly Toolchain
```bash
rustup toolchain install nightly-2025-06-23
```

### Build from Source

1. **Clone the repository**
```bash
git clone https://github.com/snowmead/rust-docs-mcp.git
cd rust-docs-mcp
```

2. **Build the project**
```bash
cargo build --release -p rust-docs-mcp
```

3. **Install the binary**
```bash
# Using the built-in install command
./target/release/rust-docs-mcp install --target-dir ~/.local/bin

# Or copy manually
cp target/release/rust-docs-mcp ~/.local/bin/
```

4. **Add to PATH (if needed)**
```bash
# Add this to your shell configuration file (.bashrc, .zshrc, etc.)
export PATH="$HOME/.local/bin:$PATH"
```

### Platform-Specific Notes

#### macOS
The binary needs to be code signed to prevent Gatekeeper warnings:
```bash
# Remove quarantine attributes
xattr -cr ~/.local/bin/rust-docs-mcp

# Ad-hoc sign the binary
codesign --force --deep -s - ~/.local/bin/rust-docs-mcp
```

#### Windows
On Windows, the binary will be named `rust-docs-mcp.exe`. Make sure the installation directory is in your PATH.

## Claude Code Integration

### Adding as MCP Server
After installation, you can add `rust-docs-mcp` to Claude Code:

```bash
claude mcp add rust-docs -s user $(which rust-docs-mcp) -t stdio
```

### Enabling All Tools
To use all available tools, add these to your Claude Code settings allow list (`~/.claude/settings.json`):

```json
{
  "allowedTools": [
    "mcp__rust-docs__cache_crate_from_cratesio",
    "mcp__rust-docs__cache_crate_from_github",
    "mcp__rust-docs__cache_crate_from_local",
    "mcp__rust-docs__remove_crate",
    "mcp__rust-docs__list_cached_crates",
    "mcp__rust-docs__list_crate_versions",
    "mcp__rust-docs__get_crates_metadata",
    "mcp__rust-docs__list_crate_items",
    "mcp__rust-docs__search_items",
    "mcp__rust-docs__search_items_preview",
    "mcp__rust-docs__get_item_details",
    "mcp__rust-docs__get_item_docs",
    "mcp__rust-docs__get_item_source",
    "mcp__rust-docs__get_dependencies",
    "mcp__rust-docs__structure"
  ]
}
```

## Verification

Test your installation:

```bash
# Check version
rust-docs-mcp --version

# Test MCP server startup (will wait for MCP client)
rust-docs-mcp
```

## Troubleshooting

### Common Issues

#### "rust-docs-mcp: command not found"
- Ensure the installation directory is in your PATH
- Reload your shell: `source ~/.bashrc` or `source ~/.zshrc`

#### "rustup: command not found"
- Install Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- Reload your shell after installation

#### macOS Gatekeeper Warnings
- Run the code signing commands from the macOS section above
- Or allow the binary in System Preferences > Security & Privacy

#### Windows Antivirus False Positives
- Add the installation directory to your antivirus whitelist
- Some antivirus software may flag the binary as suspicious

#### Build Failures
- Ensure you have the correct nightly toolchain: `rustup toolchain install nightly-2025-06-23`
- Update your Rust installation: `rustup update`

### Getting Help

If you encounter issues:

1. Check the [GitHub Issues](https://github.com/snowmead/rust-docs-mcp/issues) page
2. Create a new issue with:
   - Your operating system and version
   - Installation method used
   - Full error messages
   - Output of `rust-docs-mcp --version` (if working)

## Alternative Installation Methods

### Docker (Future Enhancement)
Docker installation will be available in future releases.

### System Package Managers (Future Enhancement)
Support for `apt`, `yum`, `pacman`, and other system package managers is planned.

## Updates

### Automatic Updates
- **Homebrew**: `brew upgrade rust-docs-mcp`
- **Scoop**: `scoop update rust-docs-mcp`

### Manual Updates
- Rebuild from source with the latest code

## Uninstallation

### Package Manager Uninstalls
- **Homebrew**: `brew uninstall rust-docs-mcp`
- **Scoop**: `scoop uninstall rust-docs-mcp`

### Manual Uninstall
```bash
# Remove binary
rm ~/.local/bin/rust-docs-mcp

# Remove from Claude Code
claude mcp remove rust-docs

# Remove cache (optional)
rm -rf ~/.cache/rust-docs-mcp
```