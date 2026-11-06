#!/usr/bin/env bash
# 离线分析抓取到的前端资源 —— 不联网，不发任何请求
#
# 用法: ./analyze.sh
# 产物: out/report.md

set -euo pipefail

OUT_DIR="${1:-out}"
REPORT="$OUT_DIR/report.md"

if [ ! -d "$OUT_DIR" ]; then
  echo "错误: 找不到 $OUT_DIR，请先运行 js_recon 抓取" >&2
  exit 1
fi

# 关注的关键词 → 说明
# 顺序即报告中的小节顺序
KEYWORDS=(
  "login_slogin|登录页在流程中的真实角色"
  "index_initMenu|登录成功后的落地页"
  "lyiotlogin|SSO 入口"
  "ticketlogin|票据登录中转"
  "kaptcha|验证码接口与参数"
  "loginUserToken|自定义请求头，需逆向生成逻辑"
  "loginToken|自定义请求头"
  "publicKey|RSA 公钥"
  "modulus|RSA 模数"
  "exponent|RSA 指数"
  "JSESSIONID|会话 Cookie"
  "SF_cookie|网关 Cookie"
  "CASTGC|认证平台 Cookie"
  "gnmkdm|功能模块菜单代码"
)

{
  echo "# 前端资源离线分析报告"
  echo
  echo "生成时间: $(date '+%Y-%m-%d %H:%M:%S')"
  echo "扫描目录: \`$OUT_DIR\`"
  echo
  echo "> 本报告由 \`analyze.sh\` 离线生成，未发起任何网络请求。"
  echo

  # ---- 文件清单 ----
  echo "## 文件清单"
  echo
  printf '| 文件 | 大小 |\n|---|---|\n'
  find "$OUT_DIR/pages" "$OUT_DIR/scripts" -type f 2>/dev/null | sort | while read -r f; do
    size=$(wc -c < "$f")
    printf '| `%s` | %s |\n' "${f#"$OUT_DIR"/}" "$size"
  done
  echo

  # ---- 接口路径 ----
  echo "## 发现的接口路径"
  echo
  echo "按出现次数排序，优先关注高频路径。"
  echo
  grep -rhoE '"/(xtgl|xsxxxggl|kbcx|jwglxt|kbgl|xygl)[A-Za-z0-9_/.]*\.(html|do|json)[^"]*"' \
    "$OUT_DIR/pages" "$OUT_DIR/scripts" 2>/dev/null \
    | tr -d '"' \
    | sed 's/[?].*//' \
    | sort | uniq -c | sort -rn | head -60 \
    || echo "（未发现）"
  echo

  # ---- 关键词命中 ----
  echo "## 关键词命中"
  echo
  for entry in "${KEYWORDS[@]}"; do
    kw="${entry%%|*}"
    desc="${entry##*|}"
    hits=$(grep -rl "$kw" "$OUT_DIR/pages" "$OUT_DIR/scripts" 2>/dev/null || true)
    if [ -n "$hits" ]; then
      echo "### \`$kw\` — $desc"
      echo
      echo "命中文件:"
      echo "$hits" | sed "s|^$OUT_DIR/||" | sed 's/^/  - `/; s/$/`/'
      echo
      echo '```'
      # 每文件最多展示 3 行上下文
      echo "$hits" | while read -r f; do
        rel="${f#"$OUT_DIR"/}"
        echo "--- $rel ---"
        grep -n "$kw" "$f" 2>/dev/null | head -3 | cut -c1-220
      done
      echo '```'
      echo
    fi
  done

  # ---- 请求头相关 ----
  echo "## 自定义请求头"
  echo
  echo '```'
  grep -rhoE "(headers|setRequestHeader|ajaxSetup)[^;]{0,200}" \
    "$OUT_DIR/scripts" 2>/dev/null | head -30 || echo "（未发现）"
  echo '```'
  echo

  # ---- 跳转逻辑 ----
  echo "## 跳转逻辑"
  echo
  echo '```'
  grep -rhoE "(location\.(href|replace)|window\.open)[^;]{0,160}" \
    "$OUT_DIR/scripts" 2>/dev/null | head -40 || echo "（未发现）"
  echo '```'

} > "$REPORT"

echo "报告已生成: $REPORT"
echo
echo "建议阅读顺序:"
echo "  1. 文件清单 —— 找 login / index / common / 体积最大的"
echo "  2. 接口路径 —— 核对项目里已实现的接口是否正确"
echo "  3. 关键词命中 —— 关注 loginUserToken 与 RSA 参数"
