# 0002. 四 crate 拓扑与依赖方向

- 状态：accepted
- 日期：2026-10-24
- 相关：architecture/00-overview.md、architecture/10-contracts.md

## 背景

产品形态分三期：CLI（一期）→ GUI（二期）→ 网络服务（三期，独立仓库）。这要求 core 是可被多种壳复用的纯库。同时，网站内容的"获取"与"转换"必须可独立替换——未来服务可能自带采集实现。若 fetch 为了实现 core 内定义的 trait 而依赖 core，会把 comrak/syntect/epub-builder 整个转换依赖栈拖进采集库，方向反了。

## 决策

workspace 分四 crate：

- **trans-epub-contract**：全部跨 crate 类型与 trait（Source/Vfs/Fetcher/IR/Frontend/Writer/ProgressSink/BuildError/ResolvedConfig），仅轻量依赖（serde/url/thiserror 级）；
- **trans-epub-core**：转换内核（三前端、pass 链、epub3 写出器、Converter 门面）；
- **trans-epub-fetch**：默认 Fetcher 实现 + 页面发现（robots/限速/sitemap/同域递归）；
- **trans-epub-cli**：薄壳（装配、配置合并、I/O 注入、落盘、报告）。

依赖红线：**core 永不依赖 fetch**；contract 不依赖任何兄弟 crate；壳层是唯一组装点。契约归属判据：会被 core 之外的 crate 实现或消费的进契约 crate；Pass trait 与 DOM 类型留 core（避免 scraper 成为公共依赖）。

## 后果

- 多一个 crate 的结构成本，换来依赖隔离与开闭扩展：新输入格式/新 pass/新输出互不波及。
- fetch 可被未来服务单独复用为底层礼貌客户端，或被服务自带实现整体替换。
