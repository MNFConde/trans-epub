---
type: adr
status: accepted
summary: "许可选型 AGPL-3.0-only：收费服务的护城河放服务端独立仓，AGPL 防第三方拿库搭闭源竞争服务；版权人保留双许可自由；only 不 or-later 钉死版本"
tags: [trans-epub, adr, license, agpl]
contains: [decision]
created: 2026-10-25
updated: 2026-10-25
related: ["../requirements.md", "../../README.md", "0013-distribution-github-releases.md"]
authoring_mode: ai_generated
---

# 0012. 许可选型：AGPL-3.0-only

- 状态：accepted
- 日期：2026-10-25
- 相关：根 `LICENSE`、workspace `Cargo.toml` license 字段、`README.md` License 节

## 背景

workspace 此前无许可（`Cargo.toml` 注释"license 待定：先自用，保留开源可能"）。规划演变：项目将以 GitHub Releases 公开分发二进制（ADR 0013），且后续会基于本项目搭建**部分收费的商业服务**——分发在即，许可必须先定。

## 决策

**AGPL-3.0-only**。`LICENSE` 收官方英文全文；`[workspace.package] license = "AGPL-3.0-only"` 四 crate 继承；README License 节说明网络服务义务与商业服务边界。

## 依据

- 商业服务的护城河放在**服务端独立仓库**（计费、账号、调度等，不在本仓库）；CLI/库以传播与贡献为目标。AGPL 恰好补上 MIT/Apache 缺的一块：防止第三方拿库稍作修改就搭**闭源**竞争服务（AGPL 的网络服务条款要求改版服务同样开源）。
- 版权人对自有代码不受 AGPL 约束：将来商业服务需要闭源组件时可行使双许可。**变更许可需要所有贡献者同意**——当前零外部贡献者，是换证成本最低的窗口，尽早钉死。
- `only` 而非 `or-later`：版本钉死更可控；将来是否升级到新版 AGPL 是版权人单方可决定的事，无需现在让渡。
- 依赖兼容性核查：全部依赖为 MIT / Apache-2.0 / ISC / BSD 系，与 AGPL 分发兼容，无阻塞。

## 后果

- GitHub Releases 分发二进制的对应源码义务，由同 tag 的源码树满足（release notes 注明即可）。
- 公司用户可能因 AGPL 合规顾虑回避本工具——接受（目标用户是个人技术书读者与作者）。
- 将来若上架 crates.io，包元数据已就位，无需补证。
- 贡献指南若将来引入，需说明 CLA/贡献授权与双许可的关系（届时另立决策）。
