#!/usr/bin/env bash
# GNNUHub 构建脚本（沙箱/只读 HOME 环境适配）
#
# 背景：DSH 沙箱内 ~/.local、~/.config、~/.cache 只读，Flutter / Gradle /
# Android SDK 都需要可写目录。脚本把全部状态重定向到工作区内的 .home 与
# .tmp/android-sdk（sdk 主体用符号链接复用系统已装组件，NDK 与 CMake 由
# 本脚本按需下载安装）。
set -euo pipefail
cd "$(dirname "$0")/.."

source "$(dirname "$0")/flutter-env.sh"

mode="${1:-both}"
case "$mode" in
  debug)   flutter build apk --debug ;;
  release) flutter build apk --release ;;
  linux)   flutter build linux --release ;;
  both)
    flutter build apk --debug
    flutter build apk --release
    ;;
  *)
    echo "用法: bash scripts/build.sh [debug|release|linux|both]" >&2
    exit 2
    ;;
esac
