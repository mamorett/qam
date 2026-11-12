# Homebrew Release Guide

Because `qam` has zero external dependencies, the Homebrew formula does **not** depend on `ffmpeg`:

```ruby
class Qam < Formula
  desc "One command to grab RTSP frames, clips, or motion alerts"
  homepage "https://github.com/steipete/qam"
  version "0.4.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/steipete/qam/releases/download/v0.4.0/qam_0.4.0_darwin_arm64.tar.gz"
      sha256 "..."
    else
      url "https://github.com/steipete/qam/releases/download/v0.4.0/qam_0.4.0_darwin_amd64.tar.gz"
      sha256 "..."
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/steipete/qam/releases/download/v0.4.0/qam_0.4.0_linux_arm64.tar.gz"
      sha256 "..."
    else
      url "https://github.com/steipete/qam/releases/download/v0.4.0/qam_0.4.0_linux_amd64.tar.gz"
      sha256 "..."
    end
  end

  def install
    bin.install "qam"
  end

  test do
    assert_match "qam 0.4.0", shell_output("#{bin}/qam --version")
  end
end
```
