---
type: docs
status: active
summary: "安装指南：GitHub Releases 预编译二进制（win/linux/mac × x64/arm64）、cargo install --git、源码构建（Rust 1.85+）三种方式"
tags: [trans-epub, guide, install]
created: 2026-10-25
updated: 2026-10-25
related: ["README.md", "cli.md"]
authoring_mode: ai_generated
---

# 安装

三种方式任选；命令用法见[命令参考](cli.md)。

## 预编译二进制（推荐）

从 GitHub 仓库的 [Releases](https://github.com/) 页下载对应平台的压缩包，解压即得单个可执行文件 `trans-epub`，无运行时依赖。

| 平台 | 目标 |
|---|---|
| Windows | x64、arm64 |
| Linux | x64、arm64（静态链接，无 glibc 版本要求） |
| macOS | x64、arm64 |

> Releases 首发前（见 [todo](../../todo.todo)），请先用下面两种方式。

## cargo 从源码安装

```bash
cargo install --git <repo-url> trans-epub-cli
```

（仓库地址首发后补全。）本地需要 Rust 工具链。

## 从仓库构建

```bash
git clone <repo-url>
cd trans-epub
cargo build --release
# 产物：target/release/trans-epub
```

需要 Rust 1.85+（版本由仓库根 `rust-toolchain.toml` 钉定，rustup 会自动安装对应工具链）。
