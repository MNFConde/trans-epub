//! 前端契约：probe / load 分离（ADR-0010）。
//!
//! probe 只做识别与章节清单（Outline），不做内容转换——dry-run、GUI
//! 结构预览、服务的"先确认目录再出书"都只消费 Outline；load 才产出
//! 完整 BookDraft。前端优先级 = Converter 注册顺序（默认
//! mdbook → md → web），多命中取第一个，其余作为信息呈现。

use serde::Serialize;

use crate::config::ResolvedConfig;
use crate::error::{BuildError, Warning};
use crate::ir::{BookDraft, SourceKey};
use crate::progress::ProgressSink;
use crate::source::Source;

/// 源归一器：把一种源格式归一为 [`BookDraft`]（前端义务清单见
/// docs/architecture/10-contracts.md）。
pub trait Frontend: Send + Sync {
    /// 稳定标识："md" / "mdbook" / "web" / ...
    fn name(&self) -> &'static str;
    /// 能否识别该源；能识别则给出章节清单预览。带配置：结构声明覆盖
    ///（声明 > 探测）在 probe 就要生效，dry-run 才能反映真实书序。
    fn probe(&self, source: &Source, config: &ResolvedConfig) -> Probe;
    /// 归一化：源 → BookDraft。顺序与嵌套在此定死。
    fn load(&self, source: &Source, env: &LoadEnv<'_>) -> Result<BookDraft, BuildError>;
}

/// probe 结果。
#[derive(Debug, Clone)]
pub enum Probe {
    NoMatch,
    Matched { outline: Outline },
}

/// 章节清单预览（dry-run / GUI 结构预览 / 服务确认目录）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Outline {
    /// 结构种类："md-tree" / "md-flat" / "mdbook" / "web" / ...
    pub kind: &'static str,
    pub items: Vec<OutlineItem>,
    pub warnings: Vec<Warning>,
}

/// Outline 节点。`source` 同时是 GUI 勾选剔除的定位键。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct OutlineItem {
    pub title: String,
    pub source: SourceKey,
    pub children: Vec<OutlineItem>,
}

/// load 的环境。
pub struct LoadEnv<'a> {
    pub config: &'a ResolvedConfig,
    pub progress: &'a dyn ProgressSink,
}
