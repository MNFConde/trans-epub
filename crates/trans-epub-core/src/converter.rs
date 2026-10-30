//! Converter 门面：装配（前端注册序 / pass 链 / writer）+ probe + convert
//! 编排（docs/architecture/20-core.md）。
//!
//! 装配扩展点即开闭原则的入口：新前端 `with_frontend`（插到最高优先
//! 级）、新 pass `with_pass`（追加链尾）、新 writer `with_writer`——
//! 已有组件零改动。I/O 全部经 [`Io`] 注入，core 不自建任何 I/O。

use std::sync::Arc;

use trans_epub_contract::config::ResolvedConfig;
use trans_epub_contract::error::BuildError;
use trans_epub_contract::frontend::{Frontend, LoadEnv, Outline, Probe};
use trans_epub_contract::ir::AssetData;
use trans_epub_contract::progress::ProgressSink;
use trans_epub_contract::source::{Fetcher, Source};
use trans_epub_contract::writer::{BuildOutput, BuildReport, BuildStats, Writer};

use crate::dom::BookDom;
use crate::frontends::md::MdFrontend;
use crate::frontends::mdbook::MdbookFrontend;
use crate::frontends::web::WebFrontend;
use crate::pipeline::{AssetResolve, CodeWrap, Fonts, Highlight, LinkRewrite, Pass, PassCtx};
use crate::reporter::Reporter;
use crate::writer::Epub3Writer;

/// 注入的 I/O，由壳层组装（CLI / 未来服务各自提供实现）。
pub struct Io {
    pub fetcher: Arc<dyn Fetcher>,
    pub progress: Arc<dyn ProgressSink>,
}

pub struct Converter {
    /// 注册顺序 = probe 优先级（默认 mdbook → md → web；web 只吃 Pages 源）。
    frontends: Vec<Box<dyn Frontend>>,
    /// 顺序 = 执行顺序（顺序契约见 20-core.md 的表）。
    passes: Vec<Box<dyn Pass>>,
    writer: Box<dyn Writer>,
}

impl Default for Converter {
    fn default() -> Self {
        Self::new()
    }
}

impl Converter {
    /// 默认装配：mdbook → md → web 前端 + 标准 pass 链 + epub3 writer。
    pub fn new() -> Self {
        Self {
            frontends: vec![
                Box::new(MdbookFrontend),
                Box::new(MdFrontend),
                Box::new(WebFrontend),
            ],
            passes: vec![
                Box::new(AssetResolve),
                Box::new(LinkRewrite),
                Box::new(Highlight),
                Box::new(CodeWrap),
                Box::new(Fonts),
            ],
            writer: Box::new(Epub3Writer),
        }
    }

    /// 注册自定义前端（插到最高优先级）。
    pub fn with_frontend(mut self, frontend: Box<dyn Frontend>) -> Self {
        self.frontends.insert(0, frontend);
        self
    }

    /// 追加自定义 pass 到链尾（插入位置约束写进 20-core.md 顺序表）。
    pub fn with_pass(mut self, pass: Box<dyn Pass>) -> Self {
        self.passes.push(pass);
        self
    }

    /// 替换 writer（新输出格式扩展点）。
    pub fn with_writer(mut self, writer: Box<dyn Writer>) -> Self {
        self.writer = writer;
        self
    }

    /// 探测：返回按优先级排序的全部命中（取第一个；其余供壳层呈现）。
    pub fn probe(&self, source: &Source, config: &ResolvedConfig) -> Vec<(String, Outline)> {
        self.frontends
            .iter()
            .filter_map(|f| match f.probe(source, config) {
                Probe::Matched { outline } => Some((f.name().to_string(), outline)),
                Probe::NoMatch => None,
            })
            .collect()
    }

    /// 转换：probe 选前端 → load → pass 链 → write → BuildOutput。
    pub fn convert(
        &self,
        source: &Source,
        config: &ResolvedConfig,
        io: &Io,
    ) -> Result<BuildOutput, BuildError> {
        io.progress.stage("probe");
        let matched = self
            .frontends
            .iter()
            .filter_map(|f| match f.probe(source, config) {
                Probe::Matched { outline } => Some((f.as_ref(), outline)),
                Probe::NoMatch => None,
            })
            .collect::<Vec<_>>();
        let Some((frontend, _)) = matched.first() else {
            return Err(BuildError::Source(
                "no frontend matched the input (mdbook / loose markdown / crawled pages supported)"
                    .into(),
            ));
        };

        io.progress.stage("load");
        let book = frontend.load(
            source,
            &LoadEnv {
                config,
                progress: io.progress.as_ref(),
            },
        )?;

        let mut dom = BookDom::from_book(book);
        let mut stats = BuildStats::default();
        // 容忍轨上报单点：pass 经 ctx.reporter 上报（实时镜像即刻发生），
        // pass 边界把批次并入权威 vec——顺序与 pass 执行序一致，两路不分叉
        let mut pass_warnings = Vec::new();
        for pass in &self.passes {
            // 链执行器在 pass 边界检查取消（每章边界由各 pass 自查）
            if io.progress.is_cancelled() {
                return Err(BuildError::Cancelled);
            }
            io.progress.stage(pass.name());
            let mut ctx = PassCtx {
                config,
                source,
                fetcher: io.fetcher.as_ref(),
                progress: io.progress.as_ref(),
                stats: &mut stats,
                reporter: Reporter::new(&mut pass_warnings, io.progress.as_ref()),
            };
            pass.run(&mut dom, &mut ctx)?;
            dom.warnings.append(&mut pass_warnings);
        }

        let book = dom.into_book();
        stats.chapters = book.chapters.len() as u64;
        stats.assets_embedded = book
            .assets
            .values()
            .filter(|a| matches!(a.data, AssetData::Owned(_)))
            .count() as u64;
        stats.assets_missing = book
            .assets
            .values()
            .filter(|a| matches!(a.data, AssetData::Missing))
            .count() as u64;

        io.progress.stage("write");
        let bytes = self.writer.write(&book, config)?;
        Ok(BuildOutput {
            format: self.writer.format().to_string(),
            bytes,
            report: BuildReport {
                warnings: book.warnings,
                stats,
            },
        })
    }
}
