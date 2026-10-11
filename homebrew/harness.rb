class Harness < Formula
  desc "Rust work assistant with terminal and local browser interfaces"
  homepage "https://github.com/seanebones-lang/harness"
  # Refresh SHA256s after tagging: bash scripts/update-homebrew-sha.sh vX.Y.Z
  version "1.4.0-beta.2"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/seanebones-lang/harness/releases/download/v#{version}/harness-macos-aarch64"
      sha256 "34593c3313a42cbed4d32fd7312a26a8e9d5adddbf0afd3a11db003b3ae1c01c"
    else
      url "https://github.com/seanebones-lang/harness/releases/download/v#{version}/harness-macos-x86_64"
      sha256 "affc74cefdb197a5d8713327819470050b14b18e1aba296d249b7533379cd46e"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/seanebones-lang/harness/releases/download/v#{version}/harness-linux-aarch64"
      sha256 "95e27cd0d52be2a93288dee0bc8f453c6e2fbb49dc50db2174be83c732770b69"
    else
      url "https://github.com/seanebones-lang/harness/releases/download/v#{version}/harness-linux-x86_64"
      sha256 "3102c1dff2069ae2c97b05315b940b201f692b73117ba0120f931e65d2582176"
    end
  end

  release_version = version
  resource "harness-license" do
    url "https://github.com/seanebones-lang/harness/releases/download/v#{release_version}/LICENSE.txt"
    sha256 "ea8f3bf6e196fc163a30dcb580af8cd4b77b9509b6f018cf8c9681ee9b7329c7"
  end

  resource "dependency-licenses" do
    url "https://github.com/seanebones-lang/harness/releases/download/v#{release_version}/THIRD-PARTY-LICENSES.html"
    sha256 "1c89ce1ef5a15d1291962786f999adf20112a8cde93e6bcf541b33a0bee80d57"
  end

  def install
    bin.install File.basename(stable.url) => "harness"
    resource("harness-license").stage { pkgshare.install "LICENSE.txt" }
    resource("dependency-licenses").stage { pkgshare.install "THIRD-PARTY-LICENSES.html" }
  end

  test do
    system "#{bin}/harness", "--version"
  end
end
