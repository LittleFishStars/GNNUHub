# GNNUHub

赣南师范大学教务系统接口的 **Flutter** 重新实现。

> 仅供学习交流使用，请自觉遵守国家相关法律法规，谨慎使用。
> 任何因使用本项目造成的后果均与作者无关。
>
> 本项目所有接口均由分析前端请求得到，作者不对其稳定性做任何保证。

## 项目定位

整个仓库是标准 Flutter 工程，按「逻辑 → 界面」分层：

- **`lib/core/`**：领域模型、错误枚举、主机常量（无网络依赖）
- **`lib/crypto/`**：复刻前端 JS 的 RSA 加密（密码与 `loginUserToken`）
- **`lib/ocr/`**：位图查表验证码识别（内嵌字库，72 条 / 62 字符）
- **`lib/api/`**：CAS 登录、SSO 三跳、请求节流、Cookie 显式管理、
  HTML/JSON 解析、会话缓存与业务接口
- **`lib/service/`**：UI 无关的应用服务层（UI 唯一入口）
- **`lib/state/`**：本地轻量持久化（记住学号与学期）
- **`lib/ui/`**：Flutter 界面（登录 + 7 个功能页，FlClash 风格）

旧的 Rust/Tauri 实现完整保留在 [`legacy/rust/`](legacy/rust/README.md)
作为参考，不参与构建。

## 界面与功能

| 页面 | 数据 | 说明 |
|---|---|---|
| 登录 | CAS + 验证码自动识别 | 验证码由内嵌字库识别，识别错自动换码重试 |
| 课表 | `xskbcx_cxXsgrkb` | 学年/学期/周次选择、周网格视图、当前周跳转 |
| 考试 | `kscx_cxXsksxxIndex` | 全量考试记录（服务端不按学期过滤） |
| 重修 | `cxkccx_cxCxkccxIndex` | 全库 4312 条，本地按名称/代码/学院过滤 |
| 通知 | `index_cxNews` | 通知列表、系统提醒、详情正文（图片内联标注） |
| 学业 | `xsxyqk_cxKczxAllIndex` | 按课程性质分组 GPA + 学分加权总绩点 + 校历 |
| 教室 | `cdjy_cxKxcdlb` | 日期/时段/楼栋/类别筛选，网格展示空闲场地 |
| 学籍 | 首页 + 学籍 JSON（失败回退 HTML） | 三源合并，字段逐次补齐 |

> 成绩查询暂不提供：成绩接口对测试账号恒定返回空信封（无法实测），
> 项目纪律是「无法实测的接口不转正」，见 `legacy/rust/logic/api/examples/probe_grade.rs`。

## 视觉规范

参照 FlClash 界面体系（[`lib/ui/theme.dart`](lib/ui/theme.dart)）：

- **填充色阶画层级，不用边框**：输入框、卡片、条件按钮全部是圆角填充块
- **颜色只给状态**：错误、置顶、补考、未开班等才用彩色容器
- **阴影只给浮层**：对话框、菜单、SnackBar 有 elevation，卡片为 0
- **大圆角**：卡片 18、输入/按钮 14、条件块 12
- **多用图标代替文字**：统一使用 MIT 许可的 Lucide 线型图标库
  （`lucide_icons_flutter`），线宽/尺寸档统一，不混用风格
- 亮/暗双主题，窄屏底部导航（`NavigationBar`）、宽屏侧边导航（`NavigationRail`）
- **无系统标题栏**（Linux 桌面）：原生壳用 `gtk_window_set_decorated(FALSE)`
  去掉 GTK/WM 标题栏，Flutter 侧只保留一条 32px 透明拖动区 + 右上角
  最小化/最大化/关闭图标按钮（[`window_manager`](https://pub.dev/packages/window_manager)），
  不显示标题文字、不画色块与边框；无边框窗口仍可用边缘拖拽缩放

## 技术选型

| 方面 | 选择 | 说明 |
|---|---|---|
| 框架 | Flutter 3.47.9 / Dart 3.13 | 一套代码覆盖 Android / Linux 桌面 |
| HTTP | `dio` | 手动管理重定向（`followRedirects=false`）与 Cookie |
| HTML | `html` | 解析服务端渲染页面（通知、学籍、校历） |
| 图像 | `image` | 验证码 PNG 解码（不做灰度、不做缩放） |
| 状态 | `provider` | `HubService` 继承 `ChangeNotifier` |
| 图标 | `lucide_icons_flutter` | MIT，统一线型 |
| 窗口 | `window_manager` | 桌面端隐藏系统标题栏、自绘窗口按钮与边缘缩放 |

### 关键移植约束（务必保留）

1. **HTTP 方法不可统一**：课表/节次/考试/重修/GPA/教室/学籍 JSON 是
   POST（query + form），首页资料/校历/通知/教学周是 GET。历史上把
   `send` 写死 GET 导致课表恒返回 `null`。
2. **Cookie 全部显式管理**：不启用 cookie jar，避免两个同名
   `JSESSIONID` 叠加导致服务端直接断开连接；`rememberMe=deleteMe`
   必须在解析阶段剔除。
3. **SSO 三跳**：第 1 跳裸请求带 `ticket`，拿到根作用域
   `JSESSIONID` 后立即停止，**不要请求 `login_slogin.html`**。
4. **请求节流 600ms**：服务端有 SafeDog WAF，突发请求会封 IP；
   节流需持锁跨等待、失败不更新时间戳。
5. **账号锁定（`USERLOCK`）绝不可重试**：继续提交只会加重锁定；
   `retrySafe(HubError)` 提供统一判据。
6. **验证码 OCR**：8 邻域连通域 + i/j 点合并 + 包围盒裁剪 +
   汉明距离 ≤2 模糊匹配。没有灰度化、没有缩放、没有平移容差搜索
   （注释与实现不一致时以代码为准）。
7. **`xqm` 约定**：第一学期 = `3`、第二学期 = `12`（不是 1/2）。
8. 表单编码用 `application/x-www-form-urlencoded`（空格→`+`），
   `lib/api/client.dart` 的 `encodeForm` 手工拼接以对齐 reqwest。

## 开发

```bash
# 首次：安装依赖
flutter pub get

# 静态分析 + 测试（60 个用例，含 RSA 对拍与验证码真实样本回归）
flutter analyze
flutter test

# 运行（Linux 桌面 / Android 设备）
flutter run -d linux
flutter run -d <android-device>

# 构建
flutter build linux --release
flutter build apk --release
```

Linux 桌面构建需要 `gtk3`、`webkit2gtk-4.1`（Arch 包名）与标准
CMake/Ninja 工具链；Android 构建需要 Android SDK。

## 分层规则

`lib/ui/` 下的实现**只允许依赖 `lib/service/`**（加上
`lib/core/model.dart` 的领域模型），不要绕过服务层直接调用
`lib/api/`——否则会话状态、登录流程与错误分类就会在每个 UI 里
各自为政、越写越散。

展示层面的决定（错误文案如何呈现、重试按钮是否可用）由 UI 自行处理；
`retrySafe` 提供统一的「重试是否有害」判据。

## 礼仪

教务系统前置了 SafeDog WAF，会因突发请求封禁来源 IP。客户端默认
对请求做 600ms 节流，**请勿关闭**，也请勿在无必要时反复登录试探接口。

## 构建环境说明（只读 HOME）

标准 Flutter 工程可直接 `flutter pub get && flutter build ...`。若 HOME 只读
（如沙箱环境），仓库附带 [`scripts/flutter-env.sh`](scripts/flutter-env.sh) 与
[`scripts/build.sh`](scripts/build.sh)：把 `HOME`、`PUB_CACHE`、
`GRADLE_USER_HOME`、`ANDROID_SDK_ROOT`、`ANDROID_USER_HOME` 全部重定向到
工作区内，并锁定 JDK 21（Gradle 9.3.1 的 Groovy 不支持 Java 27 的
class major 71）。Android SDK 镜像复用系统组件，另需 NDK 28.2.13676358、
`platforms;android-35` 与 CMake 3.22.1（分别对应 Flutter 插件默认 NDK
版本、`jni` 插件的 compileSdk 与 native 构建）。

```bash
bash scripts/build.sh            # debug + release 两套 APK
bash scripts/build.sh linux      # Linux 桌面版
```
