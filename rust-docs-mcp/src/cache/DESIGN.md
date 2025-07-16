# Cache Design Decisions

## Overview

The rust-docs-mcp cache system supports three sources for caching Rust crates:
- crates.io (published crates)
- GitHub (repository snapshots)
- Local file system (development crates)

## Key Design Decisions

### No Workspace Isolation

Initially, there was concern that cached crates might accidentally inherit workspace configuration from parent directories. However, analysis revealed this is not an issue because:

1. **Cache directory structure has no Cargo.toml** - The cache is stored in `~/.rust-docs-mcp/cache/crates/`, which contains no workspace configuration
2. **Cargo commands use explicit working directories** - All cargo/rustdoc commands run with `current_dir()` set to the cached crate's source directory
3. **Cargo only searches up from current directory** - Not from where the binary was launched

Therefore, no workspace isolation (empty `[workspace]` section) is needed, and adding it would actually break workspace crates that need their member dependencies resolved correctly.

### No Feature Conflict Detection

The system does not attempt to detect mutually exclusive features because:

1. **Cargo handles conflicts** - When features truly conflict, cargo will produce clear error messages
2. **Most conflicts are compile-time** - Mutually exclusive features typically result in compilation errors that are easy to understand
3. **Complexity without proven benefit** - Feature analysis adds significant complexity without evidence of real-world problems
4. **Documentation coverage** - Using `--all-features` by default maximizes documentation coverage for most crates

If a crate fails to build with `--all-features`, the error message from cargo is sufficient for users to understand the issue.

## Cache Structure

```
~/.rust-docs-mcp/
└── cache/
    └── crates/
        ├── standalone-crate/
        │   └── 1.0.0/
        │       ├── source/       # Extracted source code
        │       ├── docs.json     # Generated rustdoc JSON
        │       ├── metadata.json # Cache metadata
        │       └── dependencies.json
        └── workspace-crate/
            └── 1.0.0/
                ├── source/       # Workspace root
                └── members/      # Member crate docs
                    ├── member-a/
                    └── member-b/
```

## Workspace Handling

When a workspace crate is detected:
1. The system returns a list of available members
2. Users can then cache specific members by providing the member list
3. Each member's documentation is generated and stored separately
4. Member dependencies are resolved using the workspace's dependency specifications

This approach ensures that workspace members correctly inherit their intended dependencies and configuration.