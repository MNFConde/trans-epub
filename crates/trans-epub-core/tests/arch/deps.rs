//! 依赖方向红线（docs/architecture/10-contracts.md 分层）：
//! contract ← core ← {fetch, cli}。直接解析各 crate 的 Cargo.toml，
//! 不引入额外工具。

use std::collections::BTreeSet;

/// crate 的 workspace 内部依赖集合（`trans-epub-*` 前缀过滤）。
fn internal_deps(crate_name: &str) -> BTreeSet<String> {
    let manifest = format!(
        "{}/../../crates/{crate_name}/Cargo.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    // 整份文档须走 serde API（toml 1.x 的 Value::from_str 只收单个值
    // 表达式），与 src/frontends 解析 book.toml 的方式一致
    let cargo: toml::Value = toml::from_str(
        &std::fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("read {manifest}: {e}")),
    )
    .unwrap_or_else(|e| panic!("parse {manifest}: {e}"));
    cargo
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .map(|deps| {
            deps.keys()
                .filter(|k| k.starts_with("trans-epub-"))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn core_never_depends_on_fetch() {
    // core 依赖 fetch = 离线转换被网络栈拖累，分层倒退
    assert!(
        !internal_deps("trans-epub-core").contains("trans-epub-fetch"),
        "core 不得依赖 fetch：contract ← core ← {{fetch, cli}}（10-contracts.md）"
    );
}

#[test]
fn internal_dependency_graph_matches_layers() {
    assert_eq!(
        internal_deps("trans-epub-contract"),
        BTreeSet::new(),
        "contract 是最底层，不得依赖任何内部 crate"
    );
    assert_eq!(
        internal_deps("trans-epub-core"),
        BTreeSet::from(["trans-epub-contract".to_string()])
    );
    assert_eq!(
        internal_deps("trans-epub-fetch"),
        BTreeSet::from(["trans-epub-contract".to_string()])
    );
    assert_eq!(
        internal_deps("trans-epub-cli"),
        BTreeSet::from([
            "trans-epub-core".to_string(),
            "trans-epub-fetch".to_string(),
        ])
    );
}
