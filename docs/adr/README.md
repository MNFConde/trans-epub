---
type: docs
status: active
summary: "ADR 索引：一篇一文件 NNNN-slug 连续编号，格式/工作流/状态语义（proposed→accepted→superseded）与索引表"
tags: [trans-epub, adr, map]
contains: [checklist]
created: 2026-10-24
updated: 2026-10-25
related: ["../README.md"]
authoring_mode: ai_generated
---

# 架构决策记录（ADR）

## 是什么

每条重要决策一页纸：为什么、定了什么、后果。架构文档（[../architecture/00-overview.md](../architecture/00-overview.md)）描述**现状是什么**；ADR 记录**为什么走到这里**——决策一经写入不再改写，被推翻时标 superseded 并指向替代 ADR，正文保持原样。

## 格式

文件名：`NNNN-英文短标题.md`（编号递增，永不复用）。

```markdown
# NNNN. 标题

- 状态：proposed | accepted | superseded by NNNN | deprecated
- 日期：YYYY-MM-DD
- 相关：受影响的文档/代码

## 背景
## 决策
## 后果
```

## 工作流

1. 新决策：先建 ADR（proposed → accepted），再动代码；涉及契约的决策同步更新 ../architecture/10-contracts.md。
2. 推翻旧决策：新建 ADR，旧的改为 `superseded by NNNN`，旧正文不改写。
3. 编号只增不减；日期用决策实际发生的日期（回填的注明回填）。

## 索引

| # | 决策 | 状态 | 日期 |
|---|---|---|---|
| [0001](0001-rename-to-trans-epub.md) | 项目更名 mdepub → trans-epub | accepted | 2026-10-24 |
| [0002](0002-crate-topology-and-dependency-direction.md) | 四 crate 拓扑与依赖方向 | accepted | 2026-10-24 |
| [0003](0003-two-source-shapes.md) | 输入收敛为 Vfs / Pages 两形态 | accepted | 2026-10-24 |
| [0004](0004-sync-core-io-injection.md) | 同步内核 + I/O 全注入 | accepted | 2026-10-24 |
| [0005](0005-mdbook-eat-src-not-build-output.md) | mdBook 吃 src/ 原始文件而非 build 产物 | accepted | 2026-10-10 |
| [0006](0006-build-time-syntax-highlighting.md) | 构建期 syntect 高亮 | accepted | 2026-10-10 |
| [0007](0007-code-long-line-strategy.md) | 代码长行：pre-wrap 默认 + 可选硬折行 | accepted | 2026-10-10 |
| [0008](0008-epub3-only-no-kindle.md) | EPUB3 唯一输出，不做 Kindle 适配 | accepted | 2026-10-10 |
| [0009](0009-one-shot-stateless-conversion.md) | 一次性全量转换，无缓存无增量 | accepted | 2026-10-10 |
| [0010](0010-probe-load-split.md) | probe / load 分离 | accepted | 2026-10-24 |

> 0005–0009 定稿于 2026-10-10 需求采访，2026-10-24 拆分时回填为 ADR。
