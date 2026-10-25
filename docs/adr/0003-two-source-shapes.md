---
type: adr
status: accepted
summary: "输入收敛为 Vfs / Pages 两形态"
tags: [trans-epub, adr]
contains: [decision]
created: 2026-10-24
updated: 2026-10-24
related: ["../architecture/10-contracts.md", "0002-crate-topology-and-dependency-direction.md"]
authoring_mode: ai_generated
---

# 0003. 输入收敛为 Vfs / Pages 两形态

- 状态：accepted
- 日期：2026-10-24
- 相关：architecture/10-contracts.md（输入侧契约）、ADR-0002

## 背景

已知输入有四种：散装 markdown、mdBook 项目、静态文档站、未来的外置渲染器产物（SPA 预渲染）。若按格式逐一定义输入接口，消费方就要对输入类型做 enum 全处 match——每加一种格式都要改所有消费方，破坏开闭原则。

## 决策

全部输入收敛为两种源形态：

- **`Source::Vfs`**（文件树世界）：散装 md、mdBook 项目消费；
- **`Source::Pages`**（页面世界）：web 前端、外置渲染器消费。输入是**已抓取**的页面列表（url + html），顺序即书序。

抓取永远发生在 core 之外（fetch crate / 未来服务）；core 不含任何爬取逻辑，web 前端只做正文抽取归一。

## 后果

- 采集质量（页面发现与排序）的责任清晰落在壳层；core 对 SPA 的预留接缝零新增抽象即可接入。
- Vfs 由壳层实现（CLI=磁盘、服务=内存/对象存储），输入中立同时方便测试。
