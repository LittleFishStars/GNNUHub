#!/usr/bin/env bash
# Flutter 构建环境（本文件入库；运行时状态目录 .home / .tmp 由 gitignore 忽略）
#
# 用途：只读 HOME 环境（如 DSH 沙箱）下，把 Flutter / Dart / Gradle /
# Android SDK 需要的可写状态全部重定向到仓库内。
# 常规开发环境可忽略本文件，直接用系统 Flutter。
#
# 用法：source scripts/flutter-env.sh

_GNNUHUB_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Flutter SDK：优先用系统安装的；没有则用 .tmp/flutter（由 SDK tarball 解压）
if command -v flutter >/dev/null 2>&1; then
  export FLUTTER_ROOT="$(dirname "$(dirname "$(readlink -f "$(command -v flutter)")")")"
elif [[ -x "$_GNNUHUB_ROOT/.tmp/flutter/bin/flutter" ]]; then
  export FLUTTER_ROOT="$_GNNUHUB_ROOT/.tmp/flutter"
else
  echo "未找到 Flutter SDK，请先安装或把 SDK 解压到 .tmp/flutter" >&2
  return 1 2>/dev/null || exit 1
fi

# HOME / 缓存全部落到仓库内（沙箱下 ~/.config、~/.local 只读）
export HOME="$_GNNUHUB_ROOT/.home"
mkdir -p "$HOME"
export PUB_CACHE="$HOME/.pub-cache"
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_CACHE_HOME="$HOME/.cache"
export XDG_DATA_HOME="$HOME/.local/share"
export GRADLE_USER_HOME="$HOME/.gradle"

# Android SDK：系统目录只读时用仓库内镜像（符号链接复用系统组件，
# 另装 NDK 28.2.13676358 / platforms;android-35 / CMake 3.22.1）
if [[ -d "$_GNNUHUB_ROOT/.tmp/android-sdk/ndk" ]]; then
  export ANDROID_SDK_ROOT="$_GNNUHUB_ROOT/.tmp/android-sdk"
  export ANDROID_HOME="$ANDROID_SDK_ROOT"
  export ANDROID_USER_HOME="$HOME/android-user"
  mkdir -p "$ANDROID_USER_HOME"
  # Gradle 9.3.1 的 Groovy 不支持 Java 27（class major 71）
  if [[ -x /usr/lib/jvm/java-21-openjdk/bin/java ]]; then
    export JAVA_HOME=/usr/lib/jvm/java-21-openjdk
  fi
fi

export PATH="${JAVA_HOME:+$JAVA_HOME/bin:}$FLUTTER_ROOT/bin:$PATH"
