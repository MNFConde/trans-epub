//! trans-epub-fetch：礼貌采集库（docs/architecture/40-fetch.md）。
//!
//! 职责一句话：把"礼貌地拿到一批已排序页面"做成可复用能力。
//!
//! - [`polite::PoliteFetcher`]：契约 [`Fetcher`](trans_epub_contract::source::Fetcher)
//!   的默认实现——robots.txt、限速、超时重试、大小上限。
//! - [`discover`]：页面发现三机制（URL 列表 / sitemap.xml / 同域递归 BFS），
//!   产出有序 `Vec<FetchedPage>`，**顺序即书序**。
//!
//! 红线：依赖方向只能是 fetch → contract，绝不依赖 core；core 也永远不知道
//! fetch 的存在（采集可整体替换）。
//!
//! 合规：本库面向**个人学习用途的文档化采集**；robots.txt 遵守是默认行为
//!（仅显式开关可关，自负其责）、默认限速温和、不伪装浏览器 UA。使用者对
//! 目标站点的条款负责。

pub mod discover;
pub mod polite;

pub use discover::{DiscoverCfg, Discovered};
pub use polite::{PoliteConfig, PoliteFetcher};
