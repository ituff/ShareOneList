#!/bin/bash
#
# ShareOneList macOS Gatekeeper helper
# Removes the quarantine attribute from ShareOneList.app and opens it.
#
# Usage:
#   ./fix-macos-gatekeeper.command
#   ./fix-macos-gatekeeper.command /path/to/ShareOneList.app
#
set -u

APP_NAME="ShareOneList.app"

find_app() {
  local path="$1"
  if [ -n "$path" ] && [ -d "$path" ]; then
    printf '%s' "$path"
    return 0
  fi

  local candidates=(
    "/Applications/${APP_NAME}"
    "${HOME}/Applications/${APP_NAME}"
    "${HOME}/Downloads/${APP_NAME}"
    "${HOME}/Desktop/${APP_NAME}"
  )

  local candidate
  for candidate in "${candidates[@]}"; do
    if [ -d "$candidate" ]; then
      printf '%s' "$candidate"
      return 0
    fi
  done

  return 1
}

APP_PATH="$(find_app "${1:-}")"
if [ -z "$APP_PATH" ]; then
  echo "未找到 ${APP_NAME}，请先把应用拖入 /Applications，或手动指定路径："
  echo "  $0 /完整/路径/ShareOneList.app"
  exit 1
fi

echo "正在处理: ${APP_PATH}"

if xattr -cr "$APP_PATH" 2>/dev/null; then
  echo "已移除隔离属性，可以正常打开了。"
else
  echo "当前用户权限不足，尝试使用 sudo 移除隔离属性（请输入你的密码）："
  if sudo xattr -cr "$APP_PATH"; then
    echo "已通过 sudo 移除隔离属性，可以正常打开了。"
  else
    echo "无法自动移除隔离属性，请手动执行："
    echo "  sudo xattr -cr \"${APP_PATH}\""
    exit 1
  fi
fi

echo "正在打开 ShareOneList..."
open "$APP_PATH"
