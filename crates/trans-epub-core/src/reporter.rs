//! 故障上报收敛点（10-contracts.md「故障上报：三接收点统一」）。
//!
//! 权威汇总（`BuildReport.warnings`）与实时镜像（[`ProgressSink::warning`])
//! 从同一处出发——[`Reporter::report`] 先镜像、后入账，两路同源不分叉。
//! 各组件不再直接操作 warnings vec；core 不碰 stderr / UI / 网络，
//! 出口适配是壳层的事（CLI stderr 行、GUI 回调投递、服务流式透传）。

use trans_epub_contract::error::Warning;
use trans_epub_contract::progress::ProgressSink;

/// 单点出口：收集权威 warnings vec + 同时刻镜像推 [`ProgressSink::warning`]。
pub struct Reporter<'a> {
    sink: &'a dyn ProgressSink,
    warnings: &'a mut Vec<Warning>,
}

impl<'a> Reporter<'a> {
    pub fn new(warnings: &'a mut Vec<Warning>, sink: &'a dyn ProgressSink) -> Self {
        Self { sink, warnings }
    }

    /// 登记一条容忍轨故障：镜像推壳层 + 权威入账，同一时刻、同一顺序。
    pub fn report(&mut self, warning: Warning) {
        self.sink.warning(&warning);
        self.warnings.push(warning);
    }

    /// 并入一批已攒的局部 warning（前端内部 accumulator 的合并口径）：
    /// 逐条走 [`Reporter::report`]，镜像顺序与入账顺序一致。
    pub fn absorb(&mut self, batch: impl IntoIterator<Item = Warning>) {
        for warning in batch {
            self.report(warning);
        }
    }
}

/// [`Reporter::report`] 的自由函数形：helper 只拿到裸 `&mut Vec<Warning>`
///（如 `book.warnings`）而无处安放带生命周期的 Reporter 时，临时构造
/// 报告一条。与 [`Reporter::report`] 同一逻辑，不产生第二实现。
pub fn report(warnings: &mut Vec<Warning>, sink: &dyn ProgressSink, warning: Warning) {
    sink.warning(&warning);
    warnings.push(warning);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use trans_epub_contract::error::WarningCode;
    use trans_epub_contract::progress::NoProgress;

    /// 记录收到镜像事件的 sink。
    struct RecordingSink {
        seen: Mutex<Vec<Warning>>,
    }

    impl ProgressSink for RecordingSink {
        fn stage(&self, _name: &str) {}
        fn item(&self, _done: u64, _total: u64, _note: &str) {}
        fn warning(&self, w: &Warning) {
            self.seen.lock().unwrap().push(w.clone());
        }
    }

    fn warn(code: WarningCode, message: &str) -> Warning {
        Warning::new(code, None, message)
    }

    #[test]
    fn report_mirrors_and_records_same_moment() {
        let sink = RecordingSink {
            seen: Mutex::new(Vec::new()),
        };
        let mut warnings = Vec::new();
        let mut reporter = Reporter::new(&mut warnings, &sink);
        reporter.report(warn(WarningCode::LinkUnresolved, "a"));
        reporter.report(warn(WarningCode::AssetFetchFailed, "b"));

        assert_eq!(warnings.len(), 2, "权威 vec 收齐");
        assert_eq!(sink.seen.lock().unwrap().len(), 2, "镜像逐条推送");
        assert_eq!(
            *sink.seen.lock().unwrap(),
            warnings,
            "两路同源：内容与顺序一致"
        );
    }

    #[test]
    fn absorb_keeps_batch_order() {
        let sink = RecordingSink {
            seen: Mutex::new(Vec::new()),
        };
        let mut warnings = Vec::new();
        let mut reporter = Reporter::new(&mut warnings, &sink);
        reporter.absorb([
            warn(WarningCode::StructureAmbiguous, "first"),
            warn(WarningCode::StructureAmbiguous, "second"),
        ]);

        let messages: Vec<_> = warnings.iter().map(|w| w.message.as_str()).collect();
        assert_eq!(messages, ["first", "second"]);
        assert_eq!(*sink.seen.lock().unwrap(), warnings);
    }

    #[test]
    fn free_function_matches_reporter_logic() {
        let sink = RecordingSink {
            seen: Mutex::new(Vec::new()),
        };
        let mut warnings = Vec::new();
        report(&mut warnings, &sink, warn(WarningCode::EncodingLossy, "x"));

        assert_eq!(warnings.len(), 1);
        assert_eq!(sink.seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn default_sink_receives_nothing_and_vec_still_grows() {
        let mut warnings = Vec::new();
        let mut reporter = Reporter::new(&mut warnings, &NoProgress);
        reporter.report(warn(WarningCode::DuplicateSource, "quiet"));

        assert_eq!(warnings.len(), 1, "默认空实现的镜像不影响权威入账");
    }
}
