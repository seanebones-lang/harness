class Harness < Formula
  desc "Fast, private, terminal-first Rust coding agent"
  homepage "https://github.com/seanebones-lang/harness"
  version "0.1.1-beta"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/seanebones-lang/harness/releases/download/v#{version}/harness-macos-aarch64"
      sha256 "REPLACE_WITH_ACTUAL_SHA"
    else
      url "https://github.com/seanebones-lang/harness/releases/download/v#{version}/harness-macos-x86_64"
      sha256 "REPLACE_WITH_ACTUAL_SHA"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/seanebones-lang/harness/releases/download/v#{version}/harness-linux-aarch64"
      sha256 "REPLACE_WITH_ACTUAL_SHA"
    else
      url "https://github.com/seanebones-lang/harness/releases/download/v#{version}/harness-linux-x86_64"
      sha256 "REPLACE_WITH_ACTUAL_SHA"
    end
  end

  def install
    bin.install "harness"
  end

  test do
    system "#{bin}/harness", "--version"
  end
end
