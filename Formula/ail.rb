class Ail < Formula
  desc "Unified CLI/TUI for managing AI coding agent sessions"
  homepage "https://github.com/jake-hong/ail"
  url "https://github.com/jake-hong/ail/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "2fc035b1c253ade79d45ac251b9c7c12efa38b8d735418c43a2b1e9373bbb126"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "ail", shell_output("#{bin}/ail --version")
  end
end
