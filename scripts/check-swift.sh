#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only AND LicenseRef-Commons-Clause-1.0
# Copyright (C) 2026 Doctor-yoi
#
# 生成示例工程，并用[swiftui] platforms中每个Apple系统的SDK做Swift类型检查
# 用法：scripts/check-swift.sh [示例目录]

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
example="${1:-$root/examples/schedule}"

cargo build --quiet --manifest-path "$root/Cargo.toml" -p veyra-cli
"$root/target/debug/veyra" -C "$example" build

# 读取[swiftui]段中的platforms，默认为ios
platforms="$(awk '/^\[swiftui\]/{s=1;next} /^\[/{s=0} s && /^platforms/{print}' "$example/veyra.toml" | grep -oE '"[a-z]+"' | tr -d '"' || true)"
platforms="${platforms:-ios}"

files=()
while IFS= read -r -d '' f; do
  files+=("$f")
done < <(find "$example/ios" -name '*.swift' -print0)

arch="$(uname -m)"
for os in $platforms; do
  case "$os" in
    ios) sdk=iphonesimulator; target="$arch-apple-ios17.0-simulator" ;;
    macos) sdk=macosx; target="$arch-apple-macos14.0" ;;
    watchos) sdk=watchsimulator; target="$arch-apple-watchos10.0-simulator" ;;
    tvos) sdk=appletvsimulator; target="$arch-apple-tvos17.0-simulator" ;;
    visionos) sdk=xrsimulator; target="$arch-apple-xros1.0-simulator" ;;
    *) echo "未知的系统：${os}" >&2; exit 1 ;;
  esac
  # 运行环境没有安装对应平台时跳过
  if ! sdk_path="$(xcrun --sdk "$sdk" --show-sdk-path 2>/dev/null)"; then
    echo "跳过${os}：未安装${sdk} SDK"
    continue
  fi
  xcrun swiftc -typecheck \
    -sdk "$sdk_path" \
    -target "$target" \
    -swift-version 6 \
    -parse-as-library \
    "${files[@]}"
  echo "Swift类型检查通过：${os}，${#files[@]}个文件"
done
