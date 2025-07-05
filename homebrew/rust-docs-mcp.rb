class RustDocsMcp < Formula
  desc "MCP server for comprehensive Rust crate documentation analysis"
  homepage "https://github.com/snowmead/rust-docs-mcp"
  url "https://github.com/snowmead/rust-docs-mcp.git", branch: "main"
  version "0.1.0-dev"
  license "MIT"
  head "https://github.com/snowmead/rust-docs-mcp.git", branch: "main"

  depends_on "rust" => :build
  depends_on "git"

  def install
    # Install the specific nightly toolchain required
    system "rustup", "toolchain", "install", "nightly-2025-06-23"
    
    # Set the toolchain for this build
    system "rustup", "override", "set", "nightly-2025-06-23"
    
    # Build the project
    system "cargo", "build", "--release", "-p", "rust-docs-mcp"
    
    # Install binary
    bin.install "target/release/rust-docs-mcp"
    
    # Handle macOS code signing to prevent Gatekeeper issues
    if OS.mac?
      system "codesign", "--force", "--deep", "-s", "-", bin/"rust-docs-mcp"
    end
    
    # Clean up toolchain override
    system "rustup", "override", "unset"
  end

  def caveats
    <<~EOS
      rust-docs-mcp is an MCP server for Rust documentation analysis.
      
      To use with Claude Code, add it as an MCP server:
        claude mcp add rust-docs -s user #{bin}/rust-docs-mcp -t stdio
      
      For more information, see: #{homepage}
    EOS
  end

  test do
    assert_match "rust-docs-mcp", shell_output("#{bin}/rust-docs-mcp --version")
    
    # Test that the binary can start (will exit quickly without MCP client)
    # Using a more portable approach since timeout command may not be available
    pid = fork { exec "#{bin}/rust-docs-mcp" }
    sleep 2
    Process.kill("TERM", pid) rescue nil
    Process.wait(pid) rescue nil
  end
end