---
type: architecture
status: active
summary: "trans-epub-cli 薄壳设计：职责清单（clap 参数/配置合并 TOML<front matter<flags/Source 构造/Io 接线/dry-run/落盘/报告）、命令面草案（path 与 crawl 两族）、退出码约定、服务化对照；零业务逻辑，全部可平移为服务的对应物"
tags: [trans-epub, architecture, cli, clap, config-merge]
contains: [design]
created: 2026-10-24
updated: 2026-10-24
related: ["../requirements.md", "10-contracts.md", "00-overview.md"]
authoring_mode: ai_generated
---

# CLI 薄壳（trans-epub-cli）

## 定位

薄壳，**零业务逻辑**：不含任何转换/探测/抓取的判断，只做装配与注入。判断"CLI 是否越界"的标准很简单——删掉 CLI 换成 HTTP handler，逻辑还能不能整体搬进服务？能，就是薄壳；不能，就是越界（该下沉 core 或 fetch）。

## 职责清单

1. **参数解析**（clap derive）→ 覆盖层。
2. **配置合并**：产出 `ResolvedConfig`。优先级（需求定稿）：**CLI flags > front matter > TOML 配置文件**；结构层面**显式声明 > 自动探测**。注意两层合并的分工：书级配置（标题/作者/主题/资产策略…）由 CLI 合并；章节级 front matter（title/order）由前端在 load 时应用——CLI 只保证把两级配置的优先级语义传对。
3. **Source 构造**：本地路径 → `FsVfs`（契约 `Vfs` 的第一个实现：目录整树 walkdir 排序遍历；单文件输入时根 = 其父目录、walk 只返回该文件，资产引用照常解析）；`crawl` 子命令 → fetch crate 采集 → `Source::Pages`（MVP3）。
4. **Io 接线**：MVP1 内置最小 Fetcher（reqwest blocking 薄包装 + native-tls（Windows=schannel，免 C 构建依赖）、UA/超时/单文件大小上限，MVP3 平移进 fetch crate 换成 PoliteFetcher）；终端 ProgressSink（stage 行 + indicatif 按章进度条，非 tty 自动隐藏；Ctrl-C 置取消位触发协作式取消）。
5. **dry-run**：跑 probe，树状打印 Outline（标题 + 来源 + 命中的前端 + kind + 探测 warning），不出书。
6. **执行与落盘**：`Converter::convert` → 写 `.epub` 文件（默认输出 = 当前目录/{输入名}.epub，`-o` 覆盖）。
7. **报告呈现**：warning 汇总人读输出（stderr）；`--report json` 输出机器可读的 BuildReport（stdout，服务/CI 用）；`--strict` 把任何 warning 视为失败。

## 命令面（MVP1 已实现）

```text
# 路径族：散装 md（MVP1）；mdBook / 文档站随 MVP2 / MVP3
trans-epub <PATH> [-o out.epub] [--dry-run]

# 通用覆盖（与 ResolvedConfig 字段一一对应，不另造概念）
    -c, --config <FILE>       # TOML 配置文件
    --title --author(可重复) --lang --cover
    --no-remote-images        # AssetsCfg.remote=false
    --wrap pre-wrap|hard [--wrap-width 92]
    --extra-css <FILE>
    --report text|json [--strict]
    -q, --quiet               # 静默（进度与文本报告）

# 采集族（MVP3）
trans-epub crawl <start-url | sitemap.xml | list.txt> [--depth N] ...
```

参数面与 `ResolvedConfig` 字段一一对应是刻意约束：CLI 不暴露任何过程性开关（"跳过某 pass"之类），扩展行为走配置语义而非命令行特例。

## 退出码

| 码 | 含义 |
|---|---|
| 0 | 成功（允许有 warning；`--strict` 下 warning 也非零） |
| 1 | 转换致命失败（BuildError）或 `--strict` 下的 warning |
| 2 | 用法/环境错误（参数不合法、路径不存在、无前端可识别） |

## 服务化对照（三期）

CLI 的每项职责在服务里的对应物见 00-overview.md 的接缝表。要点：**core/fetch/contract 届时零改动**——服务只是把"读磁盘"换成"收上传"、把"进度条"换成"SSE 推送"的另一个壳。MVP1 起 CLI 就按此纪律写，服务化就不是重构而是新增。

## 测试

- 已落地：lib 单测（参数解析 / 配置合并优先级 / clap 面自检）+ e2e（真实二进制 × fixtures：退出码、产物、dry-run、`--report json` 结构、`--strict`），全程不访问网络。
- 端到端集成测试覆盖 dry-run 输出、退出码、`--report json` 结构（tests/cli.rs）。
- crawl 测试（MVP3）：本地 mock 服务，与 fetch crate 共用测试设施，不做外网请求。
