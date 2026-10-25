# scripts/ 目录约定

> 作用范围：仅在本目录（scripts/）新增/修改脚本时生效。

- 持久化、本地运行的脚本一律放本目录；台账（每脚本一行用途/用法）见 [scripts.md](scripts.md)——**动手前先查台账**。
- 触发方式统一为 `bash scripts/<name>.sh`（或注明其他运行时）；跨平台注意 Git Bash 可用。
- **临时脚本三次法则**：同一功能的分析/验证逻辑第 3 次出现必须固化进本目录 + 台账登记；固化后只准扩展既有脚本，禁止另写散落 tmp 的同类脚本。一次性探针在台账「临时探针台账」记一行，跨会话累计 3 次同样升级。
- 脚本用法/状态记台账（随代码走）；脚本背后的技术教训按 cairn 规则沉淀（见 `cairn/RULES.md`）。

## 经验与坑（执行时一眼可见；详情指针进 cairn）

### 1. KOReader 没有 Windows 构建
- 坑：老栈阅读器抽验原计划 KOReader，但其近两年 release/nightly/官网均无 win 包。
- 对策：老栈格子用 SumatraPDF（MuPDF，同为非 web 重排引擎）便携版顶替，`-appdata` 重定向数据。
- 校验：需要 KOReader（crengine）口径时走 Android 模拟器/WSLg，不假设 Windows 直跑。
- 详情：`cairn/epub-reader-testing.md`（2026-10-25）。

### 2. Readest 桌面版要用 WebView2 官方调试口驱动
- 坑：源码里的 `READEST_CDP_PORT` 是 Linux CEF 门控；合成 `.click()` 能触发处理器但不触发 foliate 链接导航。
- 对策：`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"` 启动；内容链接用 CDP `Input.dispatchMouseEvent` 真实事件；正文在 shadow DOM iframe 里要递归穿透。
- 校验：`curl --noproxy "*" http://127.0.0.1:9222/json/list` 有 target 即通。
- 详情：`cairn/epub-reader-testing.md`（2026-10-25）。

### 3. PowerShell 窗口截图的两个坑
- 坑：`Process.MainWindowHandle` 可能指向 0×0 辅助窗口；含中文注释 + LF 行尾的 .ps1 在 Windows PowerShell 5.1 解析失败。
- 对策：按 PID 枚举可见顶层窗口取真身（宽高 >10 过滤）；脚本用 ASCII 注释 + CRLF。另：只截目标窗口矩形，全屏截图会拍进无关内容。
- 校验：截图返回非零尺寸且标题正确。
- 详情：`cairn/epub-reader-testing.md`（2026-10-25）。

### 4. node_repl kernel 无 fetch/WebSocket
- 坑：ZCode 的 js kernel 剥掉全局 fetch/WebSocket，CDP 连接直接写会报 `fetch is not defined`。
- 对策：`node:net` + `node:crypto` 手写 RFC6455 WebSocket 帧层（见临时探针台账 cdp-driver.mjs）；HTTP 用 `node:http`。
- 校验：握手返回 101 且能收发 JSON 帧。
- 详情：`cairn/epub-reader-testing.md`（2026-10-25）。
