# 0001. 项目更名 mdepub → trans-epub

- 状态：accepted
- 日期：2026-10-24
- 相关：ADR-0002、todo.todo（MVP1 改名任务）

## 背景

项目 2026-10-10 立项时定名 mdepub，需求定稿与 cargo workspace 脚手架提交均使用旧名。随后仓库更名为 trans-epub，但 crate 名（mdepub-core / mdepub-cli）与文档（docs/mdepub.md）仍是旧名；且新架构引入契约与获取两个新 crate，需要一次性统一命名。

## 决策

项目统一更名为 trans-epub；crate 按 ADR-0002 拓扑命名：trans-epub-contract / trans-epub-core / trans-epub-fetch / trans-epub-cli。发布前确认 crates.io 名称可用性（todo.todo：发布前）。

## 后果

- 待办：crate 目录与 Cargo.toml 改名、ir.rs 迁入契约 crate（todo.todo MVP1）。
- git 历史与需求基线中出现的 mdepub 原名保留不改写——历史即历史。
- 原需求文档 docs/mdepub.md 拆分为 requirements / research / ADR 后整体退场（本 ADR 即其决策内容的归宿之一）。
