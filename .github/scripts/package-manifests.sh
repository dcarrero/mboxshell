#!/usr/bin/env bash
# Writes the Homebrew formula and the Scoop manifest for one release.
#
#   package-manifests.sh <version> <assets-dir> <out-dir>
#
# <assets-dir> holds the release binaries as published (mboxshell-macos-aarch64,
# mboxshell-windows-x86_64.exe, ...). Writes <out-dir>/mboxshell.rb and
# <out-dir>/mboxshell.json. The release workflow copies them into
# dcarrero/homebrew-tap (Formula/) and dcarrero/scoop-bucket (bucket/).
set -euo pipefail

version="${1#v}"
assets="$2"
out="$3"
base="https://github.com/dcarrero/mboxshell/releases/download/v${version}"

sha() {
  local f="$assets/$1"
  [ -f "$f" ] || { echo "missing asset: $1" >&2; exit 1; }
  if command -v sha256sum >/dev/null; then sha256sum "$f" | cut -d' ' -f1
  else shasum -a 256 "$f" | cut -d' ' -f1; fi
}

mkdir -p "$out"

cat > "$out/mboxshell.rb" <<EOF
# Written by the release workflow of dcarrero/mboxshell. Do not edit by hand.
class Mboxshell < Formula
  desc "Fast terminal viewer for MBOX files of any size"
  homepage "https://github.com/dcarrero/mboxshell"
  license "MIT"

  on_macos do
    on_arm do
      url "${base}/mboxshell-macos-aarch64"
      sha256 "$(sha mboxshell-macos-aarch64)"
    end
    on_intel do
      url "${base}/mboxshell-macos-x86_64"
      sha256 "$(sha mboxshell-macos-x86_64)"
    end
  end

  on_linux do
    on_arm do
      url "${base}/mboxshell-linux-aarch64"
      sha256 "$(sha mboxshell-linux-aarch64)"
    end
    on_intel do
      url "${base}/mboxshell-linux-x86_64"
      sha256 "$(sha mboxshell-linux-x86_64)"
    end
  end

  def install
    bin.install Dir["mboxshell-*"].first => "mboxshell"
    chmod 0755, bin/"mboxshell"
    with_env(MBOXSHELL_LANG: "en") do
      generate_completions_from_executable(bin/"mboxshell", "completions")
      (man1/"mboxshell.1").write Utils.safe_popen_read(bin/"mboxshell", "manpage")
    end
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/mboxshell --version")
    (testpath/"test.mbox").write <<~MBOX
      From sender@example.com Mon Jan  1 00:00:00 2024
      From: Sender <sender@example.com>
      To: rcpt@example.com
      Subject: Hello
      Date: Mon, 1 Jan 2024 00:00:00 +0000

      Body
    MBOX
    assert_match "\"message_count\": 1", shell_output("#{bin}/mboxshell stats #{testpath}/test.mbox --json")
  end
end
EOF

cat > "$out/mboxshell.json" <<EOF
{
    "version": "${version}",
    "description": "Fast terminal viewer for MBOX files of any size",
    "homepage": "https://github.com/dcarrero/mboxshell",
    "license": "MIT",
    "architecture": {
        "64bit": {
            "url": "${base}/mboxshell-windows-x86_64.exe#/mboxshell.exe",
            "hash": "$(sha mboxshell-windows-x86_64.exe)"
        },
        "arm64": {
            "url": "${base}/mboxshell-windows-arm64.exe#/mboxshell.exe",
            "hash": "$(sha mboxshell-windows-arm64.exe)"
        }
    },
    "bin": "mboxshell.exe",
    "checkver": "github",
    "autoupdate": {
        "architecture": {
            "64bit": { "url": "https://github.com/dcarrero/mboxshell/releases/download/v\$version/mboxshell-windows-x86_64.exe#/mboxshell.exe" },
            "arm64": { "url": "https://github.com/dcarrero/mboxshell/releases/download/v\$version/mboxshell-windows-arm64.exe#/mboxshell.exe" }
        }
    }
}
EOF
