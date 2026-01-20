# Homebrew formula for Zenvo
# To use: brew tap h-mbl/zenvo && brew install zenvo
# Or: brew install h-mbl/zenvo/zenvo

class Zenvo < Formula
  desc "Node.js environment lock, doctor & repair tool"
  homepage "https://github.com/h-mbl/zenvo"
  version "0.1.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/h-mbl/zenvo/releases/download/v#{version}/zenvo-darwin-arm64.tar.gz"
      sha256 "PLACEHOLDER_SHA256_DARWIN_ARM64"
    else
      url "https://github.com/h-mbl/zenvo/releases/download/v#{version}/zenvo-darwin-x64.tar.gz"
      sha256 "PLACEHOLDER_SHA256_DARWIN_X64"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/h-mbl/zenvo/releases/download/v#{version}/zenvo-linux-arm64.tar.gz"
      sha256 "PLACEHOLDER_SHA256_LINUX_ARM64"
    else
      url "https://github.com/h-mbl/zenvo/releases/download/v#{version}/zenvo-linux-x64.tar.gz"
      sha256 "PLACEHOLDER_SHA256_LINUX_X64"
    end
  end

  def install
    # Install main CLI
    if Hardware::CPU.arm?
      bin.install "zenvo-darwin-arm64" => "zenvo" if OS.mac?
      bin.install "zenvo-linux-arm64" => "zenvo" if OS.linux?
      bin.install "zenvo-darwin-arm64-mcp" => "zenvo-mcp" if OS.mac?
      bin.install "zenvo-linux-arm64-mcp" => "zenvo-mcp" if OS.linux?
    else
      bin.install "zenvo-darwin-x64" => "zenvo" if OS.mac?
      bin.install "zenvo-linux-x64" => "zenvo" if OS.linux?
      bin.install "zenvo-darwin-x64-mcp" => "zenvo-mcp" if OS.mac?
      bin.install "zenvo-linux-x64-mcp" => "zenvo-mcp" if OS.linux?
    end

    # Install shell completions if available
    # bash_completion.install "completions/zenvo.bash" => "zenvo"
    # zsh_completion.install "completions/_zenvo"
    # fish_completion.install "completions/zenvo.fish"
  end

  test do
    assert_match "zenvo #{version}", shell_output("#{bin}/zenvo --version")
    assert_match "zenvo-mcp", shell_output("#{bin}/zenvo-mcp --help 2>&1", 0)
  end
end
