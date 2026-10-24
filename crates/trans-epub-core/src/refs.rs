//! 相对引用解析：前端资产登记与 link-rewrite 共用的地基。
//!
//! 规则与 docs/architecture/10-contracts.md「SourceKey 规范化」一致：
//! 引用以**章节 source 所在目录**为基解析；`..` 只允许在引用里出现
//!（解析时弹出），越界即解析失败；Path 世界与 Url 世界永不互转。

use std::cmp::Ordering;

use trans_epub_contract::source::RelPath;

/// 把 href/src 拆成（目标，锚点）。锚点保留原始 percent-encoding。
pub fn split_fragment(href: &str) -> (&str, Option<&str>) {
    match href.split_once('#') {
        Some((target, frag)) => (target, Some(frag)),
        None => (href, None),
    }
}

/// 是否为外部引用（绝对 URL / 协议前缀），这类引用 link-rewrite 不动。
pub fn is_external(href: &str) -> bool {
    scheme_of(href).is_some()
}

/// 提取 scheme（`mailto:`、`data:`、`http://`…）；`//host` 形式也算外部。
pub fn scheme_of(href: &str) -> Option<&str> {
    if href.starts_with("//") {
        return Some("//");
    }
    let colon = href.find(':')?;
    let scheme = &href[..colon];
    // scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )，至少一个字母
    if scheme.is_empty() || !scheme.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    if !scheme
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
    {
        return None;
    }
    Some(scheme)
}

/// 相对路径以章节文件路径 `base` 为基解析（`base` 是文件，基目录是它的父级）。
///
/// - `..` 弹出一级，越出根即失败；
/// - 每段先做 percent-decode（`my%20file.md` → `my file.md`），解码后
///   含非法字符（`\` `:` NUL）即失败（防穿越，与 [`RelPath::new`] 同规）；
/// - 解析结果与前端资产登记走同一函数，保证键一致。
pub fn resolve_rel(base: &RelPath, href: &str) -> Option<RelPath> {
    let (target, _) = split_fragment(href);
    if target.is_empty() {
        // 纯锚点（#section），不代表文件
        return None;
    }
    // data: 等带 scheme 的不是相对路径
    if scheme_of(target).is_some() {
        return None;
    }
    let mut parts: Vec<String> = base
        .parent()
        .map(|dir| dir.as_str().split('/').map(str::to_string).collect())
        .unwrap_or_default();
    for seg in target.split('/') {
        let seg = percent_decode(seg);
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            parts.pop()?;
            continue;
        }
        if seg.contains('\\') || seg.contains(':') || seg.contains('\0') {
            return None;
        }
        parts.push(seg);
    }
    RelPath::new(&parts.join("/")).ok()
}

/// percent-decode（UTF-8 有损）；`%` 后非两位 hex 则原样保留。
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(hex) = bytes.get(i + 1..i + 3) {
                if let Ok(hex) = std::str::from_utf8(hex) {
                    if let Ok(v) = u8::from_str_radix(hex, 16) {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 自然排序：数字段按数值比较（`chapter2` < `chapter10`），其余按字节。
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut ai, mut bi) = (0usize, 0usize);
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    loop {
        let a_digit = ai < ab.len() && ab[ai].is_ascii_digit();
        let b_digit = bi < bb.len() && bb[bi].is_ascii_digit();
        if a_digit && b_digit {
            let a_start = ai;
            while ai < ab.len() && ab[ai].is_ascii_digit() {
                ai += 1;
            }
            let b_start = bi;
            while bi < bb.len() && bb[bi].is_ascii_digit() {
                bi += 1;
            }
            // 去前导零后按数值比；等值则短的（前导零少）在前，保证稳定全序
            let a_num = &a[a_start..ai];
            let b_num = &b[b_start..bi];
            let a_trim = a_num.trim_start_matches('0');
            let b_trim = b_num.trim_start_matches('0');
            match a_trim
                .len()
                .cmp(&b_trim.len())
                .then_with(|| a_trim.cmp(b_trim))
            {
                Ordering::Equal => match a_num.len().cmp(&b_num.len()) {
                    Ordering::Equal => {}
                    other => return other,
                },
                other => return other,
            }
        } else if ai >= ab.len() || bi >= bb.len() {
            return ab
                .len()
                .cmp(&bb.len())
                .then_with(|| ab.get(ai..).cmp(&bb.get(bi..)));
        } else {
            match ab[ai].cmp(&bb[bi]) {
                Ordering::Equal => {
                    ai += 1;
                    bi += 1;
                }
                other => return other,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(s: &str) -> RelPath {
        RelPath::new(s).unwrap()
    }

    #[test]
    fn resolves_against_chapter_dir() {
        let base = rel("guide/ch01.md");
        assert_eq!(
            resolve_rel(&base, "img/a.png").unwrap().as_str(),
            "guide/img/a.png"
        );
        assert_eq!(resolve_rel(&base, "../b.md").unwrap().as_str(), "b.md");
        assert_eq!(resolve_rel(&base, "./c.md").unwrap().as_str(), "guide/c.md");
        assert_eq!(
            resolve_rel(&base, "d.md#anchor").unwrap().as_str(),
            "guide/d.md"
        );
    }

    #[test]
    fn resolve_rejects_escape_and_external() {
        let base = rel("guide/ch01.md");
        assert!(resolve_rel(&base, "../../up.md").is_none());
        assert!(resolve_rel(&base, "https://x.com/a.md").is_none());
        assert!(resolve_rel(&base, "C:/x.md").is_none());
        assert!(resolve_rel(&base, "#frag").is_none());
    }

    #[test]
    fn percent_decoding_matches_plain() {
        let base = rel("ch01.md");
        assert_eq!(
            resolve_rel(&base, "my%20file.md").unwrap().as_str(),
            "my file.md"
        );
        assert_eq!(
            resolve_rel(&base, "my file.md").unwrap().as_str(),
            "my file.md"
        );
    }

    #[test]
    fn scheme_detection() {
        assert_eq!(scheme_of("https://a.b"), Some("https"));
        assert_eq!(scheme_of("mailto:a@b.c"), Some("mailto"));
        assert_eq!(scheme_of("data:image/png;base64,xx"), Some("data"));
        assert_eq!(scheme_of("//cdn/x.png"), Some("//"));
        assert_eq!(scheme_of("a/b.md"), None);
        assert_eq!(scheme_of("01-a.md"), None); // "01-a" 不是合法 scheme
        assert_eq!(scheme_of("C:/x"), Some("C")); // Windows 盘符当 scheme 处理→外部
    }

    #[test]
    fn natural_ordering() {
        let mut names = vec!["ch10.md", "ch2.md", "ch1.md", "ch-2.md"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, vec!["ch-2.md", "ch1.md", "ch2.md", "ch10.md"]);
        assert_eq!(natural_cmp("a2", "a10"), Ordering::Less);
        assert_eq!(natural_cmp("a02", "a2"), Ordering::Greater); // 前导零少者优先
    }
}
