---
type: docs
status: active
summary: "命令参考（旗标全表单一事实源）：路径族与 crawl 采集族、采集旗标与通用旗标逐条含默认值、四预设选择器链、--dry-run 两族语义、退出码、TOML 配置文件与合并优先级"
tags: [trans-epub, guide, cli, reference]
contains: [checklist]
created: 2026-10-25
updated: 2026-10-26
related: ["README.md", "../architecture/60-cli.md", "../architecture/40-fetch.md"]
authoring_mode: ai_generated
---

# 命令参考

trans-epub 只有两族命令，通用旗标两族扁平复用：

```text
trans-epub <PATH> [通用旗标]                  # 路径族：本地 markdown / mdBook
trans-epub crawl <start> [采集旗标] [通用旗标]  # 采集族：静态文档站
```

架构视角的说明（壳层职责、与配置的对应约束）见 [60-cli.md](../architecture/60-cli.md)；本页是用户视角的完整旗标参考。

## 路径族

`PATH` 是 markdown 文件，或包含 markdown 的目录。前端自动探测：含 `SUMMARY.md` 的 mdBook 项目优先，否则按散装 markdown 处理。既不给 `PATH` 也不给子命令时为用法错误（退出码 2）。

## 采集族（crawl）

`start` 的形态决定发现机制：

| start 形态 | 判作 |
|---|---|
| http(s) URL 且 path 以 `.xml` 结尾 | 远程 sitemap.xml |
| 其他 http(s) URL | 同域 BFS 起始页 |
| 本地 `.xml` 文件 | 本地 sitemap.xml |
| 其他本地文件 | URL 列表（一行一 URL，`#` 开头为注释行） |

采集行为遵循礼貌原则：默认遵守 robots.txt（RFC 9309 口径）、每 host 限速、5xx/网络错误重试、单响应大小上限；非 HTML 扩展名（图片/压缩包等）不会当作页面抓取。同域 BFS 以**起始 URL 的 host** 为界——若起始 URL 会重定向到另一域名（如 `docs.gitbook.com` → `gitbook.com`），站内链接会被判跨域而不跟进，请直接用最终域名作起点。

### 采集旗标

| 旗标 | 默认 | 说明 |
|---|---|---|
| `--depth N` | 3 | BFS 深度上限（起始页为第 0 层） |
| `--max-pages N` | 500 | 页面数安全帽 |
| `--include GLOB` | — | URL path 匹配才收录（去前导 `/` 后匹配，如 `guide/*`），可重复 |
| `--exclude GLOB` | — | URL path 命中即丢弃（同上口径），可重复 |
| `--rate-ms N` | 1000 | 每 host 相邻请求最小间隔（毫秒） |
| `--ignore-robots` | 关 | 不遵守 robots.txt（自负其责） |
| `--preset NAME` | — | 正文抽取预设，见下表 |

### 正文抽取预设

| 预设 | 站点生成器 | 正文选择器链 |
|---|---|---|
| `mkdocs` | MkDocs / Material | `.md-content` → `article` → `main` |
| `docusaurus` | Docusaurus | `article` → `main` |
| `vitepress` | VitePress / VuePress | `.vp-doc` → `main` → `article` |
| `gitbook` | GitBook | `article` → `.markdown-body` → `main` |

不指定预设时走默认链 `main` → `article` → `.content` → `.markdown-body` → `body`（`body` 兜底）。自定义选择器链用 TOML 配置的 `[web] selectors`（优先于预设，见[配置文件](#配置文件)）。代码块的 `language-*` class 一律保留——构建期语法高亮依赖它。

## 通用旗标

| 旗标 | 默认 | 说明 |
|---|---|---|
| `-o, --output FILE` | `{输入名}.epub` | 输出路径（写当前目录）。落盘为原子替换：写盘途中断电/被杀不会留下半成品 `.epub`，正式文件要么是上次的完整版、要么不存在 |
| `-c, --config FILE` | — | TOML 配置文件，见[配置文件](#配置文件) |
| `--title TEXT` | 配置/探测值 | 书名 |
| `--author TEXT` | 配置/探测值 | 作者，可重复 |
| `--lang TAG` | 配置值 | 语言（BCP-47，如 `zh-CN`） |
| `--cover REF` | 无 | 封面图：相对源根的路径或 URL |
| `--no-remote-images` | 关 | 离线模式：不拉取远程图片 |
| `--wrap MODE` | `pre-wrap` | 代码长行策略：`pre-wrap`（软换行）或 `hard`（硬折行） |
| `--wrap-width N` | 92 | 硬折行列宽（`--wrap hard` 时生效） |
| `--extra-css FILE` | — | 自定义 CSS，追加在内置主题之后 |
| `--embed-fonts` | 关 | 内嵌 TransEpub Mono 字体（Maple Mono CN 子集，OFL-1.1 义务随书；CJK 书推荐开，产物约 +2~6MB） |
| `--font-subset MODE` | `preset` | 子集模式：`preset`（内置 8105 字预子集，离线）或 `exact`（按书内字符精确子集，需联网下载钉版全量字体，失败自动回落 preset） |
| `--font-file FILE` | — | 用户字体（ttf）替代内置字体，配合 `--embed-fonts`；许可责任在配置方 |
| `--report FORMAT` | `text` | 报告格式：`text`（stderr 人读）或 `json`（stdout，供 CI/服务消费） |
| `--strict` | 关 | 任何 warning 都视为失败（退出码 1） |
| `--dry-run` | 关 | 只输出清单不出书，两族语义不同（见下） |
| `-q, --quiet` | 关 | 静默：不输出进度与文本报告 |

## --dry-run 的两族语义

- **路径族**：跑探测，树状打印章节目录（标题、来源、命中的前端）与探测 warning，不构建 EPUB。
- **crawl**：页面发现完成后打印有序 URL 清单（即书序）即停——发现本身需要抓页面，但不再做正文抽取与转换。

## 退出码

| 码 | 含义 |
|---|---|
| 0 | 成功（允许有 warning；`--strict` 下 warning 也非零） |
| 1 | 转换致命失败，或 `--strict` 下的 warning |
| 2 | 用法/环境错误：参数不合法、路径不存在、无前端可识别、采集零页面 |

## 配置文件

`-c` 指定的 TOML 文件，字段与内部 `ResolvedConfig` 一一对应（CLI 不暴露任何过程性开关，扩展行为走配置语义）。合并优先级：**命令行旗标 > 章节 front matter > TOML 配置文件**。最小示例：

```toml
[book]
title = "我的书"
authors = ["张三"]

[assets]
remote = false        # 等价 --no-remote-images

[wrap]
mode = "hard"
width = 80

[fonts]               # 等价 --embed-fonts
embed = true
subset = "exact"      # preset | exact

# 用户字体（替代内置；path 相对配置文件所在目录）
# [fonts.source]
# kind = "user"
# path = "my-mono.ttf"
```
