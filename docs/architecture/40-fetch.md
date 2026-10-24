---
type: architecture
status: active
summary: "trans-epub-fetch 获取库设计：默认 Fetcher 实现（reqwest 阻塞客户端 + robots + 限速 + 重试）、页面发现三机制（URL 列表/sitemap/同域递归）、产出 FetchedPage 列表喂 web 前端；依赖方向红线（fetch→contract，core 永不依赖 fetch）与服务化角色"
tags: [trans-epub, architecture, fetch, crawler, robots, sitemap]
contains: [design]
created: 2026-10-24
updated: 2026-10-24
related: ["../requirements.md", "10-contracts.md", "00-overview.md"]
authoring_mode: ai_generated
---

# 获取库（trans-epub-fetch）

## 定位与红线

fetch 是**采集库**：把"礼貌地拿到一批已排序页面"做成可复用能力。

- 依赖方向**只能是 fetch → contract**：fetch 实现契约的 `Fetcher`、产出 `FetchedPage`，绝不依赖 core（红线见 00-overview.md）。这样 fetch 保持轻量，也能被未来服务单独拿来当底层 HTTP 客户端。
- **core 永远不知道 fetch 的存在**。core 只见 `Fetcher` trait 与 `Source::Pages`；换实现（服务自带、测试 mock）零成本。
- MVP3 成型。MVP1 期间 CLI 内置最小 Fetcher（reqwest 薄包装，仅够 asset-resolve 拉图片），接口即契约的 `Fetcher`，届时整体平移进本 crate。

## 能力面

### 默认 Fetcher 实现（PoliteFetcher）

- reqwest 阻塞客户端（core 同步内核的配套；服务复用时同样跑在 `spawn_blocking` 里）。
- **robots.txt**：按 host 缓存解析结果，禁抓的 URL 直接返回错误（调用方降级为 warning）。
- **限速**：默认每 host 1 req/s，可配；全局并发上限。
- **UA**：`trans-epub/<version> (+document conversion tool; personal use)`，可配。
- 超时与重试：默认超时 30s，重试 2 次带退避；4xx 不重试，5xx 与网络错误重试。
- 大小上限：流式读取，超 `Content-Length`/上限即截断报错（与 AssetsCfg 上限呼应，防滥用）。

### 页面发现（产出 `Vec<FetchedPage>`，顺序即书序）

三种机制对应需求定稿，全部实现：

| 机制 | 顺序语义 | 适用 |
|---|---|---|
| **URL 列表文件**（`list.txt`，一行一 URL） | 文件行序 = 书序 | 精确控序，质量最高的成书方式 |
| **sitemap.xml** | 文件内条目序 = 书序 | 站点自带结构 |
| **同域递归**（BFS） | 发现序（层序）= 书序 | 兜底；深度上限、include/exclude 路径模式、锚点剔除、URL 去重均可配 |

- 采集时只保留 `text/html`；非页面资源跳过并报告。
- 采集结果可落盘为中间产物（URL 列表 / 页面快照目录），供重排后二次转换——**排序是网站成书的质量命门**，让用户能先看清单（dry-run）、改顺序、再出书。
- 编码处理在 web 前端（body 传原始字节），fetch 不做有损转换。

### 与 CLI 的接线

```
trans-epub crawl <start-url | sitemap.xml | list.txt> [--depth N] [--include GLOB] [--exclude GLOB]
```

crawl 子命令 = 发现 + 抓取 → `Vec<FetchedPage>` → `Source::Pages` → Converter。`--dry-run` 在发现完成后即打印清单停止。

### 服务化角色（三期）

服务需要的是"带队列/调度/缓存的采集系统"，那是服务的领域层；fetch crate 是它下面的**礼貌客户端库**。两种消费方式都成立：直接复用 PoliteFetcher，或实现契约 `Fetcher` 换成服务自己的实现（代理、鉴权、分布式缓存）。队列/优先级/任务持久化不在本 crate 范围。

## 合规

- robots.txt 遵守是默认行为，仅显式开关可关（`--ignore-robots`，文档标注自负其责）。
- 默认限速温和；不伪装浏览器 UA。
- README 与 `--help` 明示：面向个人学习用途的文档化采集，使用者对目标站点的条款负责。

## 测试

- 本地 mock HTTP 服务（httpmock/wiremock 级）覆盖：robots 允许/禁止/缺失、限速节拍、重试与退避、大小上限截断、sitemap 变体（index/嵌套/非标准）、BFS 深度与模式过滤、去重。
- golden：三个公开文档站各留一组录制好的 FetchedPage 快照（避免 CI 依赖外网）。
