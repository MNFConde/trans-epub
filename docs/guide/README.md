---
type: docs
status: active
summary: "用户手册入口：受众与三分钟上手（路径族/crawl/dry-run），指向安装页与命令参考页；架构与决策另走 docs 文档地图"
tags: [trans-epub, guide, entry]
created: 2026-10-25
updated: 2026-10-25
related: ["install.md", "cli.md", "../README.md"]
authoring_mode: ai_generated
---

# trans-epub 用户手册

把 markdown / mdBook / 静态文档站变成 EPUB3 电子书。本手册面向**最终用户**——只想把书转出来，不关心内部结构；架构与设计决策见[文档地图](../README.md)。

## 三分钟上手

```bash
# 1. 本地书：散装 markdown 目录或 mdBook 项目 → EPUB
trans-epub ./my-book -o my-book.epub

# 2. 先看一眼探测出的目录树与 warning，不出书
trans-epub ./my-book --dry-run

# 3. 在线文档站：采集 + 转换一步完成（mkdocs/docusaurus/vitepress/gitbook 有预设）
trans-epub crawl https://example.com/docs/ --preset mkdocs -o docs.epub
```

不确定输入会被识别成什么时，先跑 `--dry-run`。

## 页面目录

| 页面 | 内容 |
|---|---|
| [安装](install.md) | 预编译二进制 / cargo 安装 / 源码构建 |
| [命令参考](cli.md) | 两族命令、全部旗标逐条（含默认值）、预设选择器链、退出码、配置文件 |
