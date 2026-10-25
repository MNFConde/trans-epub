---
type: adr
status: accepted
summary: "probe / load 分离"
tags: [trans-epub, adr]
contains: [decision]
created: 2026-10-24
updated: 2026-10-24
related: ["../architecture/10-contracts.md", "../architecture/30-frontends.md"]
authoring_mode: ai_generated
---

# 0010. probe / load 分离

- 状态：accepted
- 日期：2026-10-24
- 相关：architecture/10-contracts.md（前端契约）、architecture/30-frontends.md

## 背景

dry-run（MVP3 需求）、GUI 结构预览（二期，用户明确要求：看到识别出的章节清单、检查多抓/漏抓、勾选剔除后出书）、服务的"先给用户确认目录再出书"——三种场景都只需要章节清单，不应付出整书转换的代价。

## 决策

Frontend 契约拆两步：**probe** 只做源识别与 Outline 产出（廉价：看文件名/URL 与少量元数据，不解析正文）；**load** 才产出完整 BookDraft。前端优先级 = Converter 注册顺序（默认 mdbook → md → web），多命中取第一个，其余作为信息呈现。

## 后果

- 每个前端必须实现两个入口（probe/load 共享结构探测代码）。
- 三种壳层（CLI/GUI/服务）共用同一探测能力；Outline 同时是 GUI 勾选剔除的定位键。
