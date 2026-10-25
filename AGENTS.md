# trans-epub 协作规则

trans-epub —— markdown / mdBook / 静态文档站 → EPUB3 转换器，差异化是"编程文档保真"（构建期语法高亮、代码长行处理、mdBook 语义、链接改写）。当前状态：MVP1（散装 md，a95bd71）、MVP2（mdBook 前端）与 MVP3（fetch 采集库 + web 前端 + CLI crawl）已落地、门禁全绿；发布准备进行中——License 定为 AGPL-3.0-only（ADR 0012）、分发定为 GitHub Releases 二进制且 crates.io 暂缓（ADR 0013，TLS 随之切 rustls+ring），用户手册 `docs/guide/` 与根 README 已就位；下一步 Releases 首发（CI 流水线 + 版本基线）与 Apple Books 真机补验。

## 阅读顺序

1. 先读本文件（项目约束起点）。
2. 仓库存在 `cairn/` 时，先读 `cairn/RULES.md`（知识沉淀规则与完成回复门）；否则跳过。
3. 目录级约束：`docs/AGENTS.md`、`scripts/AGENTS.md`，读取对应目录文件时自动生效。
4. 按需读 `docs/README.md`（文档地图）与 `cairn/ROADMAP.md`、`cairn/LOG.md`（存在时）。

## 文档协作规则

- 改动前判断用户要「讨论/建议」还是「直接改」；说"先看看/先评估"时先给分析，不直接重写正式文档。
- 纠正过往判断时追加更正说明，不悄悄覆盖。
- 未经确认的判断不写成既成事实。

## 协作约定

- 与用户交流一律使用中文。
- 指令若与仓库文档（`docs/`）或既有约定不符：先指出冲突点、说明取舍，再执行。
- 项目状态变化时同步更新本文件状态段。
- 契约/接口类事实变更：先改 `docs/architecture/10-contracts.md`，再动代码。

## 仲裁（skill 冲突裁决）

- 优先级：**项目内实际约定 > 项目级 skill > 全局 skill > 通用规范集**。
- 语言规范以 rust-* 系列 skill 为准；知识/教训沉淀唯一走 cairn（存在 `cairn/` 时）。
- **AGENTS.md 是本项目 tracked 资产**：任何 skill 只追加节、不独占文件，不得将其移出 git——隐私需求按内容拆分到 untracked 侧。

## 质量约束

- 代码门禁：`cargo fmt --check` → `cargo clippy -D warnings` → `cargo test`。
- 产物验收：epubcheck 全绿（`scripts/epubcheck.sh`）+ golden 字节回归（`UPDATE_GOLDEN=1` 再生，diff 人工确认）+ 真机抽验（`docs/testing/manual-checklist.md`）。
- 提交信息遵循 Conventional Commits（类型: 主题，正文解释为什么）；`.githooks/commit-msg` 钩子强制格式（新克隆一次性安装：`git config core.hooksPath .githooks`）。
- 文档门禁：`python scripts/check-links.py` / `check-frontmatter.py` / `check-markers.py` 对 docs/ 全绿（台账见 `scripts/scripts.md`）。
