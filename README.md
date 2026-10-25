# trans-epub

markdown / mdBook / 静态文档站 → EPUB3 转换器。差异化是**编程文档保真**：构建期语法高亮、代码长行处理、mdBook 语义、链接改写——技术书导出后代码块依旧能读。

## 安装

三种方式任选：

- **预编译二进制**（推荐）：从 [Releases](./releases) 下载对应平台压缩包，解压即用。平台矩阵：Windows / Linux / macOS × x64 / arm64。
- **cargo 从源码安装**：`cargo install --git <repo-url>`（首发后补全仓库地址）。
- **从仓库构建**：`git clone` 后 `cargo build --release`，产物在 `target/release/trans-epub`。需要 Rust 1.85+。

## 快速上手

```bash
# 1. 散装 markdown 或 mdBook 项目目录 → EPUB
trans-epub ./my-book -o my-book.epub

# 2. 先看目录树与探测结果，不出书
trans-epub ./my-book --dry-run

# 3. 静态文档站：采集 + 转换一步完成（mkdocs/docusaurus/vitepress/gitbook 预设）
trans-epub crawl https://example.com/docs/ --preset mkdocs
```

命令与旗标逐条说明见[用户手册](docs/guide/README.md)。

## 特性

- **三前端归一**：散装 markdown 目录、mdBook 项目（SUMMARY.md 结构、`include` / `rustdoc_include` / `playground` 指令展开）、静态文档站（四预设 + 自定义选择器链）。
- **构建期语法高亮**：syntect + bat 同源扩展语法集（TypeScript / TSX / TOML 等齐全），CSS 内嵌 EPUB，阅读器端零依赖。
- **代码长线处理**：默认 `pre-wrap` 软换行，可选硬折行（`--wrap hard --wrap-width 92`）。
- **链接改写**：书内互链转为 EPUB 内跳转，未入书链接绝对化保留，不再有死链。
- **礼貌采集**：robots.txt 遵守（RFC 9309 口径）、每 host 限速、超时重试、大小上限。
- **确定性输出**：同输入同配置产出字节一致的 EPUB（golden 回归保障），epubcheck 全绿。

## 文档

| 想做什么 | 去哪 |
|---|---|
| 使用转换器 | [用户手册](docs/guide/README.md)（安装、命令参考、预设说明） |
| 理解架构 / 为什么这么设计 | [docs 文档地图](docs/README.md)（架构篇 + 13 篇 ADR） |
| 阅读器兼容性抽验 | [真机抽验清单](docs/testing/manual-checklist.md) |

## 开发

```bash
cargo fmt --check && cargo clippy -D warnings && cargo test
python scripts/check-links.py && python scripts/check-frontmatter.py && python scripts/check-markers.py
```

文档写作约束见 [docs/AGENTS.md](docs/AGENTS.md)，协作规则见 [AGENTS.md](AGENTS.md)。

## License

AGPL-3.0-only（全文见 [LICENSE](LICENSE)）。基于本代码修改后对外提供网络服务，同样负有 AGPL 的源码公开义务；官方收费服务为独立部署，其服务端代码不在本仓库。
