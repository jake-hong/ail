class Ail < Formula
  desc "Unified CLI/TUI for managing AI coding agent sessions"
  homepage "https://github.com/jake-hong/ail"
  url "https://github.com/jake-hong/ail/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "6337b4dbc875539410111976c70f26d01088bc0b60fab74f7aad1eba8fd6f808"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "ail", shell_output("#{bin}/ail --version")
  end
end
