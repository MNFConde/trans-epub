---
type: architecture
status: active
summary: "trans-epub-cli 薄壳（路径族 + crawl 采集族均已落地）：职责清单（clap 参数/配置合并 TOML<front matter<flags/Source 构造/Io 接线/dry-run/落盘/报告）、命令面（crawl 输入判别与旗标、通用旗标扁平复用）、退出码约定、服务化对照；零业务逻辑，全部可平移为服务的对应物"
tags: [trans-epub, architecture, cli, clap, config-merge]
contains: [design]
created: 2026-10-24
updated: 2026-10-25
related: ["../requirements.md", "10-contracts.md", "00-overview.md"]
authoring_mode: ai_generated
---

# CLI 薄壳（trans-epub-cli）

## 定位

薄壳，**零业务逻辑**：不含任何转换/探测/抓取的判断，只做装配与注入。判断"CLI 是否越界"的标准很简单——删掉 CLI 换成 HTTP handler，逻辑还能不能整体搬进服务？能，就是薄壳；不能，就是越界（该下沉 core 或 fetch）。

## 职责清单

1. **参数解析**（clap derive）→ 覆盖层。
2. **配置合并**：产出 `ResolvedConfig`。优先级（需求定稿）：**CLI flags > front matter > TOML 配置文件**；结构层面**显式声明 > 自动探测**。注意两层合并的分工：书级配置（标题/作者/主题/资产策略…）由 CLI 合并；章节级 front matter（title/order）由前端在 load 时应用——CLI 只保证把两级配置的优先级语义传对。
3. **Source 构造**：本地路径 → `FsVfs`（契约 `Vfs` 的第一个实现：目录整树 walkdir 排序遍历；单文件输入时根 = 其父目录、walk 只返回该文件，资产引用照常解析）；`crawl` 子命令 → fetch crate 发现与抓取 → `Source::Pages` → web 前端（MVP3 已落地）。
4. **Io 接线**：Fetcher 统一用 fetch crate 的 `PoliteFetcher`（MVP1 内置的最小 reqwest 包装已按计划平移进 trans-epub-fetch；crawl 按 `--rate-ms` / `--ignore-robots` / 资产上限配置实例化）；终端 ProgressSink（stage 行 + indicatif 按章进度条，非 tty 自动隐藏；Ctrl-C 置取消位触发协作式取消）。
5. **dry-run**：跑 probe，树状打印 Outline（标题 + 来源 + 命中的前端 + kind + 探测 warning），不出书。
6. **执行与落盘**：`Converter::convert` → 写 `.epub` 文件（默认输出 = 当前目录/{输入名}.epub，`-o` 覆盖）。
7. **报告呈现**：warning 汇总人读输出（stderr）；`--report json` 输出机器可读的 BuildReport（stdout，服务/CI 用）；`--strict` 把任何 warning 视为失败。

## 命令面（路径族与采集族均已实现）

两族命令：`trans-epub <PATH> [通用旗标]`（散装 md / mdBook 项目）与 `trans-epub crawl <start-url | sitemap.xml | list.txt> [采集旗标] [通用旗标]`（采集）。**用户视角的完整旗标参考——含默认值、四预设选择器链、退出码表、TOML 配置示例——单一事实源在 [../guide/cli.md](../guide/cli.md)**，本节只留架构侧约束：

- 通用旗标与 `ResolvedConfig` 字段一一对应是刻意约束：CLI 不暴露任何过程性开关（"跳过某 pass"之类），扩展行为走配置语义而非命令行特例。
- crawl 输入判别（远程 sitemap / BFS 起始页 / 本地 sitemap / URL 列表）在壳层 `dispatch_discovery` 完成，判别结果决定 fetch crate 的发现入口。
- `--dry-run` 语义两族不同：路径族跑 probe 打 Outline；crawl 在**发现完成后**打印有序 URL 清单即停（BFS 发现本身需要抓页面）。`--preset` 直接写 `WebCfg.preset`。

## 退出码

0 成功（`--strict` 下 warning 也非零）/ 1 转换致命失败（BuildError）或 `--strict` 下的 warning / 2 用法环境错误（参数不合法、路径不存在、无前端可识别）。完整表见 [../guide/cli.md](../guide/cli.md#退出码)（单一事实源）；服务与 CI 以退出码驱动 `--report json` 的消费流程。

## 服务化对照（三期）

CLI 的每项职责在服务里的对应物见 00-overview.md 的接缝表。要点：**core/fetch/contract 届时零改动**——服务只是把"读磁盘"换成"收上传"、把"进度条"换成"SSE 推送"的另一个壳。MVP1 起 CLI 就按此纪律写，服务化就不是重构而是新增。

## 测试

- 组织（ADR 0011）：`tests/` 单 binary 分 `contract/`（cli_surface——退出码/dry-run/report json 的进程接口契约）与 `e2e/`（convert / crawl——真实产物）；跑法 `cargo test -p trans-epub-cli --test integration`（对账 9 个）。
- 已落地：lib 单测（参数解析 / 配置合并优先级 / clap 面自检 / crawl 旗标）+ e2e（真实二进制 × fixtures：退出码、产物、dry-run、`--report json` 结构、`--strict`），全程不访问网络。
- crawl e2e（MVP3 已落地）：测试内起回环迷你 HTTP 服务（两页小站），覆盖 crawl → 发现 → 抓取 → web 前端 → EPUB 的完整链路与 dry-run 清单输出、本地输入缺失的用法错误；与 fetch crate 的 mock 服务设施各自独立、互不依赖。
