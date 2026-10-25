---
type: docs
status: active
summary: "docs/ 顶层文档地图：按稳定度分组（上游需求调研/规范架构/论证 ADR/操作测试），三列索引（文档|职责|何时读）与引用规范"
tags: [trans-epub, docs, map]
contains: [checklist]
created: 2026-10-25
updated: 2026-10-25
related: ["../AGENTS.md"]
authoring_mode: ai_generated
---

# docs/ 文档地图

trans-epub 项目文档索引。所有文档为规划稿，随实现推进持续更新（文档即源码）。

## 按稳定度分组（上层稳定、下层易变；分组承载分层语义，编号只管阅读顺序）

### 上游（做什么 / 别人怎么做）

| 文档 | 职责 | 何时读 |
|---|---|---|
| [requirements.md](requirements.md) | 需求基线 v1.0：目标、非目标、验收口径 | 一切的起点 |
| [research/ecosystem.md](research/ecosystem.md) | 生态调研：md/文档站 → EPUB 工具竞品与坑位 | 选型/遇到生态问题时 |

### 规范（是什么）

| 文档 | 职责 | 何时读 |
|---|---|---|
| [architecture/00-overview.md](architecture/00-overview.md) | 架构总纲：四 crate 拓扑、依赖红线、端到端流程、其余文档索引 | 理解整体结构时 |
| [architecture/10-contracts.md](architecture/10-contracts.md) | 契约篇：公共类型/trait 契约——**变更须文档先行于代码** | 改接口/加模块前 |
| [architecture/20-core.md](architecture/20-core.md) | 转换内核：DOM 策略、四 pass 流水线、纯净红线 | 改 core 前 |
| [architecture/30-frontends.md](architecture/30-frontends.md) | 源归一前端（均已落地）：md 探测六规则、mdBook SUMMARY/指令展开、web 抽取（选择器预设 + language class 存活） | 改前端前 |
| [architecture/40-fetch.md](architecture/40-fetch.md) | 获取库（已落地）：PoliteFetcher（robots/限速/重试）、页面发现三机制 | 改采集行为前 |
| [architecture/50-writer.md](architecture/50-writer.md) | EPUB3 写出器：确定性 zip、nav/opf/封面、CSS 主题 | 改 writer 前 |
| [architecture/60-cli.md](architecture/60-cli.md) | CLI 薄壳：路径族 + crawl 采集族命令面、配置合并、FsVfs/PoliteFetcher | 改 CLI 前 |

### 论证（为什么这么选）

| 文档 | 职责 | 何时读 |
|---|---|---|
| [adr/](adr/README.md) | 决策记录 10 篇（0001-0010）+ 索引：背景/决策/依据/影响 | 遇到"为什么这么选"时 |

### 操作（怎么落地）

| 文档 | 职责 | 何时读 |
|---|---|---|
| [testing/manual-checklist.md](testing/manual-checklist.md) | 真机抽验清单：何时抽验、检查点矩阵、阅读器优先级 | 里程碑合入前/改渲染相关时 |
| [AGENTS.md](AGENTS.md) | docs/ 写作约束（frontmatter、编号、状态标记、单源、存档） | 写/改 docs 前 |

**推荐阅读顺序**（新加入者）：requirements → architecture/00 → 按编号进领域篇 → adr 按需。

## 引用规范

- 引用一律用 markdown 链接，不用裸编号；锚点按 GitHub slug 规则（标题小写、去标点、空格→`-`、中文保留）。
- 同一事实只一处详述（单一事实源），其余链接回指；确定信息（版本号等）同样单源。
- 契约类事实（接口、数据结构、CLI 面）变更：先改 [10-contracts.md](architecture/10-contracts.md)，再动代码。
- 多候选方案用状态标记 `【当前】/【备选】/【已否决】/【已替换】`（细则见 [AGENTS.md](AGENTS.md)）。
- 机器校验：`python scripts/check-links.py` / `check-frontmatter.py` / `check-markers.py` 对 docs/ 全绿（用法与台账见 [scripts.md](../scripts/scripts.md)）。
