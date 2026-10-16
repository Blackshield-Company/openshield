# frozen_string_literal: true

# Tap: Blackshield-Company/homebrew-tap
#   brew tap Blackshield-Company/tap
#   brew install openshield

class Openshield < Formula
  desc "The harness that learns. The agent that decides"
  homepage "https://github.com/Blackshield-Company/openshield"
  version "1.1.1"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/Blackshield-Company/openshield/releases/download/v1.1.1/openshield-aarch64-apple-darwin.tar.gz"
      sha256 "a616921188a9c81e4fc6a87bfb624f5ba78088b8588f78937ce050c798ac3e37"
    end

    on_intel do
      url "https://github.com/Blackshield-Company/openshield/releases/download/v1.1.1/openshield-x86_64-apple-darwin.tar.gz"
      sha256 "9f662590cef7c9004bcff3b9bf38d4f5ba5846052d56ce891731486de1c2318f"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/Blackshield-Company/openshield/releases/download/v1.1.1/openshield-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "eacbc335c6ce57fb3602e647b8d1637484dee4f6feaa610b94db11bf720bc879"
    end

    on_intel do
      url "https://github.com/Blackshield-Company/openshield/releases/download/v1.1.1/openshield-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "a278128aaff5540d0d49dd6669a3fe9b417acc28ab48b54664c0f401d698e691"
    end
  end

  def install
    bin.install "openshield"
    prefix.install "LICENSE", "README.md"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/openshield --version")
  end
end
