//! 集成测试入口（单 binary）：语义分层目录在此挂载（ADR 0011）。
//!
//! - `contract/`：fetch crate 对 core/cli 的承诺——PoliteFetcher 礼貌
//!   语义（robots/限速/重试/大小上限）与页面发现三机制。
//! - `support/`：回环 mock HTTP 服务（随机端口，不出网）。
//!
//! fetch 规模小，暂不设 behavior/arch 层；crate 长大后按 core 的分层
//! 地图补。防静默跳过：新增测试文件必须挂进对应 mod.rs。对账：
//! `cargo test -p trans-epub-fetch --test integration -- --list`（20 个）。

mod contract;
mod support;
