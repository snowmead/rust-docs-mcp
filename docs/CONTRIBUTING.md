# Contributing to rust-docs-mcp

Thank you for your interest in contributing to rust-docs-mcp! This document provides guidelines and instructions for contributing to the project.

## Development Setup

1. Clone the repository
2. Install Rust nightly toolchain: `rustup toolchain install nightly`
3. Build the project: `cargo build`
4. Run tests: `cargo test`

## Making Changes

### Code Style

- Follow Rust's official style guide
- Use `cargo fmt` before committing
- Run `cargo clippy` and address any warnings

### Testing

- Add tests for new functionality
- Ensure all tests pass before submitting a PR
- Test with real crates to verify functionality

## Updating MCP Tools

When adding or modifying tools in the MCP server, it's important to keep the documentation synchronized. Any changes to the tools list in `rust-docs-mcp/src/service.rs` must be reflected in:

1. `README.md` - The main documentation file
2. `install.sh` - The installation script that shows available tools to users

### Using the Update Command

We provide a convenient Claude Code command to automatically update these files:

```bash
/update_mcp_tools_list
```

This command will:

- Read the current tools from `rust-docs-mcp/src/service.rs`
- Update the tools list in `README.md`
- Update the tools list in `install.sh`

### Manual Update Process

If updating manually, ensure that:

1. The tool names in documentation match exactly with the function names in `service.rs`
2. Tool descriptions are consistent across all files
3. The tools are grouped logically (Cache Management, Documentation Queries, etc.)

## Code of Conduct

Please note that this project is released with a Contributor Code of Conduct. By participating in this project you agree to abide by its terms.

## Questions?

If you have questions or need help, please open an issue on GitHub or reach out to the maintainers.
