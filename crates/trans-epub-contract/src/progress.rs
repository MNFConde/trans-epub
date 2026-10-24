//! 进度与协作式取消。
//!
//! `stage` 用字符串而非枚举：自定义 pass 也要能报进度，不因扩展改
//! 契约类型（开闭原则的细节体现）。

/// 进度回调与取消检查点。
///
/// 取消是协作式的：pipeline 在 pass 边界与每章边界检查
/// [`ProgressSink::is_cancelled`]，命中即返回
/// [`crate::error::BuildError::Cancelled`]。core 自己不建线程、不建运行时。
pub trait ProgressSink: Send + Sync {
    /// 阶段切换（"probe" / "load" / 各 pass 名 / "write"）。
    fn stage(&self, name: &str);
    /// 细粒度心跳（按章/按页）。
    fn item(&self, done: u64, total: u64, note: &str);
    /// 取消检查点；返回 true 后核心尽快以 Cancelled 收尾。
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// 什么都不做的实现（测试 / 不关心进度的调用方）。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProgress;

impl ProgressSink for NoProgress {
    fn stage(&self, _name: &str) {}
    fn item(&self, _done: u64, _total: u64, _note: &str) {}
}
