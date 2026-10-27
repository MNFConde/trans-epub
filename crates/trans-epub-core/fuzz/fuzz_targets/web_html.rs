//! fuzz 目标：web 前端对**网络输入**的抽取——抓来的页面是完全不可信
//! 的任意字节（畸形 HTML / 截断 / 乱码 / 巨大嵌套），load 全链不得 panic。
//! 优先级最高的 fuzz 面（输入完全不可控），发布前跑一轮。

#![no_main]

use libfuzzer_sys::fuzz_target;
use trans_epub_contract::config::ResolvedConfig;
use trans_epub_contract::frontend::{Frontend, LoadEnv};
use trans_epub_contract::progress::NoProgress;
use trans_epub_contract::source::{FetchedPage, Source};
use trans_epub_core::frontends::web::WebFrontend;

fuzz_target!(|data: &[u8]| {
    let page = FetchedPage::from_bytes(
        "https://example.com/page.html".parse().expect("static url"),
        data.to_vec(),
        "text/html",
    );
    // Err 是正常业务结果（抽不出正文）；panic / abort / 挂死是缺陷
    let _ = WebFrontend.load(
        &Source::Pages(vec![page]),
        &LoadEnv {
            config: &ResolvedConfig::default(),
            progress: &NoProgress,
        },
    );
});
