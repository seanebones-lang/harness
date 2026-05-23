class Harness < Formula
  desc "Fast, private, terminal-first Rust coding agent"
  homepage "https://github.com/seanebones-lang/harness"
  version "0.1.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/seanebones-lang/harness/releases/download/v#{version}/harness-aarch64-apple-darwin"
      sha256 "REPLACE_WITH_ACTUAL_SHA"
    else
      url "https://github.com/seanebones-lang/harness/releases/download/v#{version}/harness-x86_64-apple-darwin"
      sha256 "REPLACE_WITH_ACTUAL_SHA"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/seanebones-lang/harness/releases/download/v#{version}/harness-aarch64-unknown-linux-gnu"
      sha256 "REPLACE_WITH_ACTUAL_SHA"
    else
      url "https://github.com/seanebones-lang/harness/releases/download/v#{version}/harness-x86_64-unknown-linux-gnu"
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