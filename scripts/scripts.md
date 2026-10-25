# scripts/ 台账

每个持久化脚本的用途与用法一行；临时探针在下方台账登记（三次法则见 [AGENTS.md](AGENTS.md)）。

## 持久化脚本

| 脚本 | 用途 | 用法 |
|---|---|---|
| [epubcheck.sh](epubcheck.sh) | 下载/复用 epubcheck 5.2.1，校验三个 golden epub（产物验收·档一） | `bash scripts/epubcheck.sh`（依赖 java；`EPUBCHECK_JAR` 可指定本地 jar） |
| [check-links.py](check-links.py) | docs/（+cairn/）链接存在性与锚点一致性校验：递归含子域，跳过存档目录与 `{{…}}` 模板令牌 | `python scripts/check-links.py`（文档门禁） |
| [check-frontmatter.py](check-frontmatter.py) | docs/ frontmatter 必填 4 项（type/status/summary/updated）与 status 枚举校验 | `python scripts/check-frontmatter.py`（文档门禁） |
| [check-markers.py](check-markers.py) | 方案状态标记校验：行内简记=链接形、块记=小标题+callout 结构 | `python scripts/check-markers.py`（文档门禁） |
| [check-commit-msg.py](check-commit-msg.py) | 提交信息 Conventional Commits 格式校验 | `.githooks/commit-msg` 钩子调用（手动：`python scripts/check-commit-msg.py <msgfile>`） |

> 检查脚本四件复制自 vibe-project-init `assets/scripts/`（纯标准库、零环境依赖）；钩子薄壳含 python 三级探测（python3→python→`py -3`），全不可用则告警放行。

## 临时探针台账

> 单/双次使用的诊断代码在此记一行（日期/内容/用途/出现次数）；跨会话累计第 3 次必须升级固化进本目录并登记到上表。

| 日期 | 内容 | 用途 | 次数 | 位置 |
|---|---|---|---|---|
| 2026-10-25 | `cdp-driver.mjs`（node:net 手写 WebSocket 客户端 + CDP 驱动，约 200 行） | 驱动 Readest WebView2 调试口做真机抽验 | 2 | `tmp/cdp-driver.mjs` |
| 2026-10-25 | `shot.ps1`（按 PID 枚举可见顶层窗口 + DPI 感知截图） | SumatraPDF 窗口截图取证 | 2 | `tmp/shot.ps1` |

> 注：tmp/ 在 .gitignore 中，重启/清理会丢；两者若再出现即为第 3 次，从本台账行升级为 scripts/ 正式脚本。
