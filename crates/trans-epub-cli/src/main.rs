//! trans-epub CLI：一条命令把散装 markdown / mdBook 项目 / 静态文档站
//! 转成 EPUB（薄壳，零业务逻辑，见 docs/architecture/60-cli.md）。

use clap::Parser;

fn main() {
    let cli = trans_epub_cli::Cli::parse();
    std::process::exit(trans_epub_cli::run(&cli) as i32);
}
