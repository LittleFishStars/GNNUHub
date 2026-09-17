# GNNUHub 项目长期备忘

赣南师范大学教务系统 Rust 重写。参考实现：`/home/ylxc/Projects/python/GNNU_API`。

## 硬性约束（来自用户明确要求，务必遵守）

- **控制请求量**：用户曾多次因 IP 被封而强烈不满。每轮对话最多 ~20 次
  网络请求；一旦发现被拦（连接被关闭、域名整体拒连）立即停止。
- **优先离线逆向**：用户明确指示「抓包返回的 js 脚本进行逆向分析，而不是
  一直尝试」。要探接口时先用 `cargo run -p gnnuhub-api --example js_recon`
  抓一次前端资源，之后全部离线分析，不要再反复打接口试探。
- **一次只改一个变量**：做实验时不要同时改多个条件。
- 真实账号测试已获授权（学号 `250710078`）。

## 工具链与代码规范

- **stable 工具链**（见 `rust-toolchain.toml`）。`rustfmt.toml` 里
  **不能**放 nightly-only 选项（`imports_granularity`、`group_imports`），
  否则被静默忽略且每次运行都报警告。
- 提交前跑这三条：
  ```bash
  cargo test --workspace --all-targets
  cargo clippy --workspace --all-targets -- -D warnings   # 必须带 -D warnings
  cargo fmt --all --check
  ```
  注意 clippy **默认跑不够**：`derivable_impls` 等 lint 只在 `-D warnings`
  下才会暴露。
- 注释与文档一律中文。
- 代码风格：`Session` 的所有 HTTP 出口收敛到私有 `send()`，新增请求方法
  走它而不是各自拼 URL。

## 已验证的关键事实（逆向 + 实测）

- **SSO 交换共 3 跳**，且**必须在 `/xtgl/login_slogin.html` 之前停止**——
  继续跟随会被服务端直接关闭 TLS 连接（`peer closed connection without
  sending TLS close_notify`）。这是稳定的行为拦截，重试无效。
- 后续请求必须同时携带 `JSESSIONID` **与** `SF_cookie_17`（网关下发）；
  只带前者会 302。`rememberMe=deleteMe` 必须剔除，带上会被断连。
- 同名 `JSESSIONID` 有作用域：`/sso` 那份只是链路中转物，最终用根作用域那份。
- `Client::cas_headers()` 带 `Host: cas.gnnu.edu.cn`，**不可跨域复用**，
  否则 Host 与 SNI 不匹配会被断连。
- `csrftoken` 对课表/学籍查询接口**不需要**。
- 验证码：**100×25 RGBA**，固定 4 位，**字母数字 + 大小写混用**，无干扰线。
  - **开了抗锯齿**：每字符 = 1 个纯色核心 + 约 30 种 AA 混色边缘（实测 130 色）。
    所以按「严格颜色相等」提取是错的，会把斜线削细；要按**颜色就近 + 保留边缘**。
  - **有噪点**：随机小点散布在图面，是**独立连通块**，用「保留 ≥8 像素的
    连通块」可稳定分离出恰好 4 个字符（60/60 张样本验证）。
  - 4 字符 x 槽位固定 `(0,20) (21,46) (47,73) (74,99)`。
  - **同一字符有 1~4 种位图**（字号随机），240 个字形里只有 **65 种不同的位图**。
  - **字符集 = `A-Z` + `a-z` + `0-9`（62 个），逐字符随机大小写**。证据：
    整图 tesseract 识别 220 字符中大写 38.6% / 小写 42.7% / 数字 18.6%；
    48/60 张图内混用大小写与数字；同一字号下存在 6~8 种不同字形（含大小写对）。
    不含标点（早期以为有 `+`，实为 `t` 的误读）。
  - 服务器**字体不属于本机 929 个可用字族**：用 Java AWT 生成 94 万条字模
    逐像素比对，含斜线/曲线的字形无一命中（最小差 7~18px），只有纯横竖笔画
    的字形（`I` `E` `F` `L` `H` `T`）巧合命中。**逐像素模板匹配此路不通。**
  - **tesseract 5.5.3 可用**（`/usr/bin/tesseract`，有 `eng`）。**整图识别
    明显优于逐字符**：逐字符 `--psm 10` 因失去上下文更差（65 字形里 21 个
    在四种缩放下结果漂移）。
  - **不要用「多配置大投票」**：低质量配置（scale 1）与高质量等权，会把
    正确答案投掉。`0014.png` 在 `gray5p8`/`gray5p13` 下都坚定给 `LD60`，
    12 配置投票只给它 2 票。正确做法是**少量高质量配置 + 全票一致判定**。
  - **已实现 `TesseractOcr`**（commit `47d3001`，feature `tesseract` 默认关闭）：
    灰度化 → LANCZOS ×5 → `--psm 8` 与 `--psm 13` 双配置 → 全票一致才输出。
    60 张样本覆盖 47/60（78.3%）。
  - **灰度化必须复现 PIL，有两个独立的坑**（各自都能改变识别结果）：
    1. **系数用 BT.601** `(299,587,114)/1000`，**不是** `image` crate
       `to_luma8` 的 BT.709 `(2126,7152,722)/10000`。高饱和色上差 10~33 阶。
    2. **舍入是四舍五入**，必须写 `(... + 500) / 1000`，纯整除会差 4/60。
    回归测试 `gray_matches_pil_on_real_sample` 用内嵌样本逐像素守着。
  - **方案对放大倍数敏感（已知弱点）**：`0024.png` 只在 scale **4~8**
    窗口内读出正确的 `0Cax`，scale 2/3/10 都退化成 `0ax`。滤波器也刚性：
    NEAREST/BOX/BILINEAR/HAMMING/BICUBIC 全部读出 `0ax`，只有 LANCZOS 正确。
- **登录接口可验验证码真值**：`parse_ticket_response` 把
  `data.code == "CODEFALSE"` 映射为 `LoginOutcome::CaptchaIncorrect`，
  与 `BadCredentials` 干净区分，因此能用真实登录判定识别对错。
- 认证平台「地址改为服务部署地址」这种提示与实际登录失败无必然关系。
- `xqm` 取值是 **3（第一学期）/ 12（第二学期）**，不是 1/2。

## 踩过的坑

- **字段 id 极易数错字符**：政治面貌的真实 id 是 `col_zzmmm`（**两个 z**，
  是「政治面貌」的拼音首字母 z-z-m-m-m），不是 `col_zzzmmm`。教训：
  一律用 `grep -o 'id="col_[^"]*"'` 机器提取，别用肉眼数。
  `tests/real_page_check.rs` 里有一条测试专门守着这一点。
- 不要给 reqwest 开 `cookie_store(true)`：会与手动设置的 COOKIE 头叠加，
  发出两个同名 `JSESSIONID`，服务端判定异常后直接关闭连接。
- `/tmp` 在工具调用之间不持久，临时脚本放项目内 `.scratch/`（已 gitignore）。
- 沙箱里 `python3 - << 'PYEOF'` 形式的 heredoc 容易被破坏，改为写脚本文件再执行。
- **不要靠肉眼看 8~13px 点阵给字形标注**：此路已失败三次，每次都有明确错误
  （把 `Z` 看成 `3`、`D` 看成 `G`、`6` 看成 `a`、`R` 看成 `b`）。低分辨率点阵
  的笔画差异不足以可靠区分字形，必须引入其他信息源（真实标注、或询问用户）。

## 待办

- 单周课表接口 `/kbcx/xskbcxMobile_cxXsKb.html` 未实测
- 学籍字段仅映射 12/84，完整对照表在 `tools/js-recon/out/verification.md` 附一
- 成绩查询未实现（`/cjcx/cjcx_cxDgXscj.html?gnmkdm=N305005`）
- 验证码自动识别**已有实现**（`TesseractOcr` + `FailoverOcr`，commit `47d3001`），
  但**真实准确率尚未用登录接口验证**，因此 `tesseract` feature 默认关闭。
  下一步：跑登录验真值；若准确率不足，考虑**多倍数（4/5/6/7）共识**
  以缓解「只在 scale 4~8 窗口内正确」这个弱点。
- 界面层框架未选（用户明确表示稍后再定）
