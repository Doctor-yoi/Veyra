#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
# Copyright (C) 2026 Doctor-yoi
#
# 生成示例工程并用iOS模拟器SDK对Swift输出做类型检查
# 用法：scripts/check-swift.sh [示例目录]

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
example="${1:-$root/examples/schedule}"

cargo build --quiet --manifest-path "$root/Cargo.toml" -p veyra-cli
"$root/target/debug/veyra" -C "$example" build

sdk="$(xcrun --sdk iphonesimulator --show-sdk-path)"
arch="$(uname -m)"
files=()
while IFS= read -r -d '' f; do
  files+=("$f")
done < <(find "$example/ios" -name '*.swift' -print0)

xcrun swiftc -typecheck \
  -sdk "$sdk" \
  -target "$arch-apple-ios17.0-simulator" \
  -swift-version 6 \
  -parse-as-library \
  "${files[@]}"

echo "Swift类型检查通过：${#files[@]}个文件"
