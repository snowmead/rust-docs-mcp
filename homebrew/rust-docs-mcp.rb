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

  def post_install
    # Check if Claude Code is installed and offer to set up integration
    if system("which claude >/dev/null 2>&1")
      ohai "Claude Code detected!"
      
      # Check if rust-docs is already configured
      if !system("claude mcp list 2>/dev/null | grep -q rust-docs")
        ohai "Adding rust-docs-mcp to Claude Code..."
        if system("claude mcp add rust-docs -s user #{bin}/rust-docs-mcp -t stdio")
          ohai "Successfully added rust-docs-mcp to Claude Code!"
          
          # Display the tool allow list
          ohai "To enable all rust-docs tools, add these to your Claude settings allow list:"
          puts "(~/.claude/settings.json or your project/local settings)"
          puts ""
          puts "  \"mcp__rust-docs__cache_crate_from_cratesio\","
          puts "  \"mcp__rust-docs__cache_crate_from_github\","
          puts "  \"mcp__rust-docs__cache_crate_from_local\","
          puts "  \"mcp__rust-docs__remove_crate\","
          puts "  \"mcp__rust-docs__list_cached_crates\","
          puts "  \"mcp__rust-docs__list_crate_versions\","
          puts "  \"mcp__rust-docs__get_crates_metadata\","
          puts "  \"mcp__rust-docs__list_crate_items\","
          puts "  \"mcp__rust-docs__search_items\","
          puts "  \"mcp__rust-docs__search_items_preview\","
          puts "  \"mcp__rust-docs__get_item_details\","
          puts "  \"mcp__rust-docs__get_item_docs\","
          puts "  \"mcp__rust-docs__get_item_source\","
          puts "  \"mcp__rust-docs__get_dependencies\","
          puts "  \"mcp__rust-docs__structure\""
        else
          opoo "Could not automatically add rust-docs-mcp to Claude Code"
          opoo "You can add it manually with:"
          puts "  claude mcp add rust-docs -s user #{bin}/rust-docs-mcp -t stdio"
        end
      else
        ohai "rust-docs-mcp is already configured in Claude Code"
      end
    end
  end

  def caveats
    s = <<~EOS
      rust-docs-mcp is an MCP server for Rust documentation analysis.
      
    EOS
    
    # Only show manual Claude Code setup if it wasn't done automatically
    if !system("which claude >/dev/null 2>&1")
      s += <<~EOS
        To use with Claude Code (when installed), add it as an MCP server:
          claude mcp add rust-docs -s user #{bin}/rust-docs-mcp -t stdio
        
      EOS
    elsif !system("claude mcp list 2>/dev/null | grep -q rust-docs")
      s += <<~EOS
        To use with Claude Code, add it as an MCP server:
          claude mcp add rust-docs -s user #{bin}/rust-docs-mcp -t stdio
        
      EOS
    end
    
    s += <<~EOS
      For more information and detailed tool configuration, see:
        #{homepage}
    EOS
    
    s
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