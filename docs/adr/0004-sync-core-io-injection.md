# 0004. 同步内核 + I/O 全注入（服务就绪约束）

- 状态：accepted
- 日期：2026-10-24
- 相关：architecture/10-contracts.md、architecture/00-overview.md（服务化接缝表）

## 背景

三期网络服务（axum）要复用 core。若 core 直连 reqwest/std::fs、自带 tokio 运行时或持有全局状态，服务将无法替换存储/缓存/限速策略，也做不了确定性输出与结果缓存。而 comrak/syntect/epub-builder 全是同步 CPU 密集库，强行异步化没有收益。

## 决策

- core 保持**同步**、不绑运行时、无全局状态；服务侧用 `spawn_blocking` 包裹执行。
- 所有 I/O 经契约 trait 注入：文件经 `Source::Vfs`，远程经 `Fetcher`，进度与取消经 `ProgressSink`（协作式取消：pass 边界与章节边界检查）。
- Writer 返回**字节**而非文件路径；warning 结构化机器可读（code + SourceKey 定位）。
- 禁止 core 读时钟/环境变量/随机数——相同输入字节级可复现。

## 后果

- 服务化 = 换壳：core/fetch/contract 零改动（overview 接缝表逐项对照）。
- 确定性使 golden 样书可做字节比对，服务可对结果缓存去重。
- 代价：CLI 与服务各自维护少量装配代码——这本就是壳的职责。
