class Ail < Formula
  desc "Unified CLI/TUI for managing AI coding agent sessions"
  homepage "https://github.com/sungeun/ail"
  url "https://github.com/sungeun/ail/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "PLACEHOLDER"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "ail", shell_output("#{bin}/ail --version")
  end
end
