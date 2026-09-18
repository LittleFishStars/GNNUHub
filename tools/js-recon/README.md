# 前端 JS 逆向分析工具

## 目的

教务系统的登录逻辑（加密方式、跳转链路、Cookie 作用域、请求头要求）
**全部写在前端 JS 里**。与其反复发请求试探，不如一次性把相关 JS 抓下来
离线阅读——不产生试探流量，也不会触发风控。

## 为什么需要这个

本项目在打通 SSO 时走了弯路：先后误判为「网络抖动」「WAF 限流」，
累计发出上百次请求并触发了按 IP 的封禁。实际上前端 JS 里就有答案。
**先读代码，再发请求。**

## 使用方式

### 1. 抓取（需要有效会话，请求量固定且很小）

```bash
# 用现有凭据登录后抓取（约 15-25 次请求，一次性完成）
# 验证码通过 captcha.png / captcha.txt 文件桥交互
GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
  cargo run -p gnnuhub-api --example js_recon_once
```

产物落在 `tools/js-recon/out/`：
- `pages/*.html` — 各页面原始 HTML
- `scripts/*.js`  — 提取并下载的 JS 文件
- `index.json`    — 抓取清单与来源对应关系

抓取时脚本会把验证码写到 `captcha.png` 并轮询等待
`captcha.txt`（你写好字符后它自动继续，**全程一个进程，会话不断**）。

### 2. 离线分析（不联网）

```bash
./analyze.sh
```

会在 `out/report.md` 生成关键词命中汇总，重点关注：

| 关键词 | 用途 |
|---|---|
| `login_slogin` | 登录页在流程中的真实角色 |
| `index_initMenu` | 登录成功后的落地页 |
| `lyiotlogin` | SSO 入口 |
| `kaptcha` | 验证码接口与参数 |
| `loginUserToken` | 自定义请求头，需逆向其生成逻辑 |
| `publicKey` / `modulus` / `exponent` | RSA 加密参数 |
| `JSESSIONID` / `SF_cookie` | Cookie 作用域相关 |
| `gnmkdm` | 各功能模块的菜单代码 |
| `/xtgl/` `/xsxxxggl/` `/kbcx/` | 接口路径 |

### 3. 接口实测验证（联网，请求数极少）

逆向得出的结论需要用真数据确认。`verify_endpoints` 示例在
**一次登录内**跑完多个待验证接口，把请求数压到最低：

```bash
GNNU_STUDENT_ID=xxx GNNU_PASSWORD='xxx' \
  cargo run -p gnnuhub-api --example verify_endpoints
```

它验证：
1. 课表接口 `/kbcx/xskbcx_cxXsgrkb.html`（含 `csrftoken` 是否必需）
2. 学籍接口 `/xsxxxggl/xsgrxxwh_cxXsGrxx.html`（字段解析）
3. 把真实 HTML 存档到 `out/pages/xsgrxx_filled.html` 供离线核对

结论见 `out/verification.md`。

### 4. 回归测试（离线）

真实页面结构已固化为夹具，改动解析逻辑后会立刻发现回归：

```bash
cargo test -p gnnuhub-api --test real_page_check
```

夹具在 `logic/api/tests/fixtures/`，**已脱敏**。

## 注意事项

- **抓取只需跑一次**。产物入库后，后续分析全部离线完成。
- JS 文件较多时优先读 `login` / `index` / `common` 命名的，以及体积最大的。
- 教务系统多用 jQuery + 拼接式 URL，搜索 `.html?` 和 `.do?` 能快速定位接口。
- 部分 JS 可能被压缩，先看有没有 `.min.js` 之外的同名未压缩版本。
- **字段 id 极易数错字符**（如政治面貌是 `col_zzmmm` 两个 z，不是三个）。
  一律用 `grep -o 'id="col_[^"]*"'` 机器提取，**别用肉眼数**。
  我在这上面栽过一次，误判成实现有 bug，白排查很久。
