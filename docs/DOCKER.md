# Docker Usage Guide

This guide covers how to use rust-docs-mcp with Docker containers for various deployment scenarios.

## Quick Start

### Pull and Run
```bash
# Pull the latest image
docker pull ghcr.io/snowmead/rust-docs-mcp:latest

# Run as MCP server (stdio interface)
docker run -i ghcr.io/snowmead/rust-docs-mcp:latest
```

### With Cache Persistence
```bash
# Create a cache directory
mkdir -p ./rust-docs-cache

# Run with persistent cache
docker run -i -v ./rust-docs-cache:/home/rustdocs/.rust-docs-mcp/cache \
  ghcr.io/snowmead/rust-docs-mcp:latest
```

## Available Images

### Tags
- `latest` - Latest release from main branch
- `v1.0.0` - Specific version tags
- `main` - Latest commit from main branch
- `<sha>` - Specific commit builds

### Supported Architectures
- `linux/amd64` (x86_64)
- `linux/arm64` (ARM64/Apple Silicon)

## Usage Examples

### Basic MCP Server
```bash
# Run as MCP server with stdio transport
docker run -i ghcr.io/snowmead/rust-docs-mcp:latest
```

### With Custom Cache Directory
```bash
# Mount a host directory for cache persistence
docker run -i \
  -v /host/cache/path:/home/rustdocs/.rust-docs-mcp/cache \
  ghcr.io/snowmead/rust-docs-mcp:latest
```

### Help and Commands
```bash
# Show help
docker run ghcr.io/snowmead/rust-docs-mcp:latest --help

# Show install command help
docker run ghcr.io/snowmead/rust-docs-mcp:latest install --help
```

### Interactive Usage
```bash
# Run with interactive terminal
docker run -it ghcr.io/snowmead/rust-docs-mcp:latest
```

## Docker Compose

### Basic Setup
```yaml
version: '3.8'

services:
  rust-docs-mcp:
    image: ghcr.io/snowmead/rust-docs-mcp:latest
    stdin_open: true
    tty: true
    volumes:
      - ./cache:/home/rustdocs/.rust-docs-mcp/cache
    environment:
      - RUST_DOCS_MCP_CACHE_DIR=/home/rustdocs/.rust-docs-mcp/cache
```

### Run with Docker Compose
```bash
# Start the service
docker-compose up rust-docs-mcp

# Run in background
docker-compose up -d rust-docs-mcp

# Stop the service
docker-compose down
```

### Development Setup
```bash
# Run development profile with source code mounted
docker-compose --profile dev up rust-docs-mcp-dev
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `RUST_DOCS_MCP_CACHE_DIR` | Cache directory path | `/home/rustdocs/.rust-docs-mcp/cache` |
| `RUST_LOG` | Log level (trace, debug, info, warn, error) | `info` |

## Volume Mounts

### Cache Directory
- **Container Path**: `/home/rustdocs/.rust-docs-mcp/cache`
- **Purpose**: Persistent storage for downloaded documentation and cached data
- **Recommended**: Always mount to avoid re-downloading on container restart

### Configuration (Optional)
- **Container Path**: `/home/rustdocs/.rust-docs-mcp/config`
- **Purpose**: Store custom configuration files
- **Usage**: Mount if you have custom settings

## Security Considerations

The container follows security best practices:

### Non-root User
- Runs as `rustdocs` user (UID 1000)
- No root privileges inside container
- Proper file permissions for cache directory

### Minimal Attack Surface
- Based on minimal Debian slim image
- Only essential runtime dependencies included
- No unnecessary packages or tools

### Security Recommendations
```bash
# Run with additional security options
docker run -i \
  --read-only \
  --tmpfs /tmp \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  -v ./cache:/home/rustdocs/.rust-docs-mcp/cache \
  ghcr.io/snowmead/rust-docs-mcp:latest
```

## Building Images

### Build Multi-architecture Images
```bash
# Set up buildx (if not already done)
docker buildx create --use

# Build and push multi-arch image
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -t ghcr.io/snowmead/rust-docs-mcp:latest \
  --push .
```

### Build Local Image
```bash
# Build for current architecture
docker build -t rust-docs-mcp .

# Build for specific architecture
docker build --platform linux/amd64 -t rust-docs-mcp:amd64 .
```

## Troubleshooting

### Permission Issues
If you encounter permission issues with mounted volumes:
```bash
# Check the container user ID
docker run ghcr.io/snowmead/rust-docs-mcp:latest id

# Fix permissions on host
sudo chown -R 1000:1000 ./cache
```

### Cache Not Persisting
Ensure you're mounting the cache directory:
```bash
# Check if volume is mounted
docker run -i -v ./cache:/home/rustdocs/.rust-docs-mcp/cache \
  ghcr.io/snowmead/rust-docs-mcp:latest ls -la /home/rustdocs/.rust-docs-mcp/
```

### Performance Issues
For better performance with large caches:
```bash
# Use volume instead of bind mount
docker volume create rust-docs-cache
docker run -i -v rust-docs-cache:/home/rustdocs/.rust-docs-mcp/cache \
  ghcr.io/snowmead/rust-docs-mcp:latest
```

## Integration Examples

### With Claude Desktop
Configure in Claude Desktop MCP settings:
```json
{
  "mcpServers": {
    "rust-docs": {
      "command": "docker",
      "args": ["run", "-i", "--rm", "-v", "./rust-docs-cache:/home/rustdocs/.rust-docs-mcp/cache", "ghcr.io/snowmead/rust-docs-mcp:latest"]
    }
  }
}
```

### With Kubernetes
```yaml
apiVersion: v1
kind: Pod
metadata:
  name: rust-docs-mcp
spec:
  containers:
  - name: rust-docs-mcp
    image: ghcr.io/snowmead/rust-docs-mcp:latest
    stdin: true
    tty: true
    volumeMounts:
    - name: cache
      mountPath: /home/rustdocs/.rust-docs-mcp/cache
    env:
    - name: RUST_DOCS_MCP_CACHE_DIR
      value: /home/rustdocs/.rust-docs-mcp/cache
  volumes:
  - name: cache
    persistentVolumeClaim:
      claimName: rust-docs-cache
```

## Best Practices

1. **Always use persistent volumes** for cache directory
2. **Pin image tags** in production (avoid `latest`)
3. **Use multi-stage builds** for smaller images
4. **Run with security constraints** in production
5. **Monitor cache size** and clean up old data periodically
6. **Use health checks** in orchestration systems

For more information, see the main [README](../README.md) and [Contributing Guide](CONTRIBUTING.md).