class Harness < Formula
  desc "Fast, private, terminal-first Rust coding agent"
  homepage "https://github.com/seanebones-lang/harness"
  # Refresh SHA256s after tagging: bash scripts/update-homebrew-sha.sh vX.Y.Z
  version "1.3.0"
  license "LicenseRef-NextEleven-Proprietary"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/seanebones-lang/harness/releases/download/v#{version}/harness-macos-aarch64"
      sha256 "bc66e7913c60e7e8562d4c18bd505989c67bd2dd67b93303a4a46b7f2948ca18"
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
