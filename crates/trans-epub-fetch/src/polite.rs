//! PoliteFetcher：契约 [`Fetcher`] 的礼貌默认实现（40-fetch.md）。
//!
//! - **robots.txt**：按 origin 缓存，遵循 RFC 9309 / Google 口径——2xx 解析
//!   （解析失败视同全放行），4xx（429 除外）全放行，429 / 5xx / 拉取失败
//!   全禁抓（站点明确异常时保守不抓）。禁抓的 URL 返回
//!   [`FetchError::Denied`]，调用方降级为 warning。
//! - **限速**：每 host（scheme+authority）最小请求间隔（默认 1s），请求槽
//!   预约制，等待发生在锁外。robots.txt 自身的拉取同占限速槽。
//! - **重试**：仅 5xx 与网络错误重试（默认 2 次、线性退避）；4xx 立即失败。
//! - **大小上限**：Content-Length 预检 + 流式读取超限即断。
//! - **UA**：默认 `trans-epub/<version> (+document conversion tool; personal use)`，
//!   可配，不伪装浏览器。

use std::collections::HashMap;
use std::io::Read as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use trans_epub_contract::source::{FetchError, FetchedPage, Fetcher};
use url::Url;

/// robots.txt 的缓存结论。`Arc` 因为 texting_robots 的 Robot 不是 Clone，
/// 且缓存命中后要移出锁使用。
#[derive(Clone)]
enum Robots {
    /// 命中规则集。
    Rules(Arc<texting_robots::Robot>),
    /// 无限制（缺失 / 4xx / 解析失败）。
    AllowAll,
    /// 全禁抓（429 / 5xx / 拉取失败）。
    DenyAll,
}

/// 礼貌抓取配置。默认值即 40-fetch.md 承诺的合规默认。
#[derive(Debug, Clone)]
pub struct PoliteConfig {
    /// User-Agent；默认带工具名与用途声明。
    pub user_agent: String,
    /// 单请求超时。
    pub timeout: Duration,
    /// 5xx / 网络错误的重试次数（总尝试 = retries + 1）。
    pub retries: u32,
    /// 重试退避基准（线性：第 n 次重试前睡 n × backoff）。
    pub retry_backoff: Duration,
    /// 每 host 相邻请求的最小间隔。
    pub min_interval: Duration,
    /// 单响应大小上限（字节）。
    pub max_bytes: u64,
    /// 是否遵守 robots.txt（关闭即 `--ignore-robots` 语义，自负其责）。
    pub respect_robots: bool,
    /// 是否走系统代理（reqwest `system-proxy`：Linux 读环境变量、Windows
    /// 读平台设置）。`false` 强制直连——回环 mock 测试必须关：代理会把
    /// 传输层故障"洗"成 502 状态码，且 env `NO_PROXY` 不认 `127.*` 通配
    /// （reqwest 上游限制，只认精确 IP / CIDR / 域名后缀）。
    pub respect_system_proxy: bool,
}

impl Default for PoliteConfig {
    fn default() -> Self {
        Self {
            user_agent: default_user_agent(),
            timeout: Duration::from_secs(30),
            retries: 2,
            retry_backoff: Duration::from_millis(500),
            min_interval: Duration::from_secs(1),
            max_bytes: 10 * 1024 * 1024,
            respect_robots: true,
            respect_system_proxy: true,
        }
    }
}

/// 默认 UA：工具名 + 用途声明（可配，不伪装）。
pub fn default_user_agent() -> String {
    format!(
        "trans-epub/{} (+document conversion tool; personal use)",
        env!("CARGO_PKG_VERSION")
    )
}

/// 单次 GET 的原始结果。
struct RawPage {
    /// 重定向后的最终 URL。
    url: Url,
    body: Vec<u8>,
    content_type: String,
}

/// 契约 `Fetcher` 的礼貌实现。
pub struct PoliteFetcher {
    client: reqwest::blocking::Client,
    config: PoliteConfig,
    robots: Mutex<HashMap<String, Robots>>,
    /// 每 host 下一次允许发请求的时刻（限速槽）。
    next_slot: Mutex<HashMap<String, Instant>>,
}

impl PoliteFetcher {
    pub fn new(config: PoliteConfig) -> Self {
        // rustls-no-provider 模式要求在构造 Client 前安装进程级 crypto provider；
        // 选 ring（纯 Rust）。Err = 进程里已装过（重复构造），按幂等容忍。
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = {
            let builder = reqwest::blocking::Client::builder()
                .user_agent(config.user_agent.clone())
                .timeout(config.timeout);
            let builder = if config.respect_system_proxy {
                builder
            } else {
                builder.no_proxy()
            };
            builder.build().unwrap_or_default()
        };
        Self {
            client,
            config,
            robots: Mutex::new(HashMap::new()),
            next_slot: Mutex::new(HashMap::new()),
        }
    }

    /// 便捷构造：默认配置 + 指定单响应大小上限（与 AssetsCfg 对齐）。
    pub fn with_max_bytes(max_bytes: u64) -> Self {
        Self::new(PoliteConfig {
            max_bytes,
            ..PoliteConfig::default()
        })
    }

    /// scheme + authority 作为限速与 robots 的 host 槽键（http/https 分槽）。
    fn host_key(url: &Url) -> String {
        format!("{}://{}", url.scheme(), url.authority())
    }

    /// 限速槽：预约本 host 的下一个时间片，返回需要等待的时长（锁外睡）。
    fn throttle(&self, host: &str) {
        let wait = {
            let mut guard = self.next_slot.lock().expect("rate lock");
            let now = Instant::now();
            let slot_start = guard.get(host).copied().unwrap_or(now).max(now);
            guard.insert(host.to_string(), slot_start + self.config.min_interval);
            slot_start.saturating_duration_since(now)
        };
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }

    /// robots.txt 检查：未缓存先拉（含限速）。被拒返回 `Err(Denied)`。
    fn check_robots(&self, url: &Url) -> Result<(), FetchError> {
        if !self.config.respect_robots {
            return Ok(());
        }
        let origin = Self::host_key(url);
        let entry = {
            let guard = self.robots.lock().expect("robots lock");
            match guard.get(&origin) {
                Some(entry) => entry.clone(),
                None => {
                    drop(guard);
                    let entry = self.load_robots(&origin);
                    self.robots
                        .lock()
                        .expect("robots lock")
                        .insert(origin.clone(), entry.clone());
                    entry
                }
            }
        };
        match entry {
            Robots::AllowAll => Ok(()),
            Robots::DenyAll => Err(FetchError::Denied(format!(
                "robots.txt unavailable with error status at {origin}; treating site as disallowed"
            ))),
            Robots::Rules(robot) => {
                if robot.allowed(url.as_str()) {
                    Ok(())
                } else {
                    Err(FetchError::Denied(format!(
                        "robots.txt disallows {url} at {origin}"
                    )))
                }
            }
        }
    }

    /// 拉取并判定某 origin 的 robots 结论（调用方负责缓存）。
    fn load_robots(&self, origin: &str) -> Robots {
        let Ok(robots_url) = format!("{origin}/robots.txt").parse::<Url>() else {
            return Robots::AllowAll;
        };
        self.throttle(origin);
        match self.raw_get(&robots_url) {
            Ok(page) => {
                // texting_robots 建议 500 KiB 截断（防超大 robots 打爆解析）
                let cap = page.body.len().min(512 * 1024);
                match texting_robots::Robot::new(&self.config.user_agent, &page.body[..cap]) {
                    Ok(robot) => Robots::Rules(Arc::new(robot)),
                    Err(_) => Robots::AllowAll, // 解析失败视同全放行（RFC 9309）
                }
            }
            Err(FetchError::HttpStatus { status, .. })
                if (400..500).contains(&status) && status != 429 =>
            {
                Robots::AllowAll // 4xx（429 除外）：站点无 robots 约束
            }
            Err(_) => Robots::DenyAll, // 429 / 5xx / 网络错误：保守不抓
        }
    }

    /// 单次 GET（限额可指定）：状态码 / Content-Length / 流式大小上限
    /// 检查；不重试不限速。
    fn raw_get_limited(&self, url: &Url, max_bytes: u64) -> Result<RawPage, FetchError> {
        let mut response = self
            .client
            .get(url.clone())
            .send()
            .map_err(|e| FetchError::Network(e.to_string()))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(FetchError::HttpStatus {
                status,
                url: url.to_string(),
            });
        }
        if let Some(len) = response.content_length() {
            if len > max_bytes {
                return Err(FetchError::TooLarge(format!(
                    "{len} bytes (limit {max_bytes}) for {url}"
                )));
            }
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let mut body = Vec::new();
        let mut limited = (&mut response).take(max_bytes + 1);
        limited
            .read_to_end(&mut body)
            .map_err(|e| FetchError::Network(e.to_string()))?;
        if body.len() as u64 > max_bytes {
            return Err(FetchError::TooLarge(format!(
                "over {max_bytes} bytes for {url}"
            )));
        }
        Ok(RawPage {
            url: response.url().clone(),
            body,
            content_type,
        })
    }

    /// 单次 GET：状态码 / Content-Length / 流式大小上限检查；不重试不限速。
    fn raw_get(&self, url: &Url) -> Result<RawPage, FetchError> {
        self.raw_get_limited(url, self.config.max_bytes)
    }
}

impl Fetcher for PoliteFetcher {
    fn fetch(&self, url: &Url) -> Result<FetchedPage, FetchError> {
        Self::ensure_http_scheme(url)?;
        self.check_robots(url)?;
        self.get_with_retry(url, self.config.max_bytes)
    }

    /// 覆写契约默认实现：用户点名单资源跳过 robots（见
    /// [`PoliteFetcher::fetch_direct_impl`]）。`max_bytes` 显式上限
    /// 取代配置默认（钉版字体 zip ~140MB 远超常规资产限额）。
    fn fetch_direct(&self, url: &Url, max_bytes: Option<u64>) -> Result<FetchedPage, FetchError> {
        self.fetch_direct_impl(url, max_bytes)
    }
}

impl PoliteFetcher {
    /// scheme 白名单：只收 http(s)（两条获取路径共用）。
    fn ensure_http_scheme(url: &Url) -> Result<(), FetchError> {
        if url.scheme() == "http" || url.scheme() == "https" {
            Ok(())
        } else {
            Err(FetchError::Denied(format!(
                "scheme not allowed: {}",
                url.scheme()
            )))
        }
    }

    /// 重试循环（两条获取路径共用）：线性退避 + 限速槽预约；
    /// 5xx 与网络错误重试，4xx / 策略拒绝 / 超限立即失败。
    /// `limit` 为本路径的单响应上限。
    fn get_with_retry(&self, url: &Url, limit: u64) -> Result<FetchedPage, FetchError> {
        let host = Self::host_key(url);
        let attempts = self.config.retries + 1;
        let mut last_err = None;
        for attempt in 1..=attempts {
            if attempt > 1 {
                // 线性退避：第 n 次重试前睡 n × backoff
                std::thread::sleep(self.config.retry_backoff * attempt);
            }
            self.throttle(&host);
            match self.raw_get_limited(url, limit) {
                Ok(page) => {
                    let mut final_url = page.url.clone();
                    final_url.set_fragment(None);
                    return Ok(FetchedPage::from_bytes(
                        final_url,
                        page.body,
                        page.content_type,
                    ));
                }
                // 4xx / 策略拒绝 / 超限：不重试
                Err(err @ FetchError::HttpStatus { status, .. })
                    if !(500..600).contains(&status) =>
                {
                    return Err(err);
                }
                Err(err @ (FetchError::Denied(_) | FetchError::TooLarge(_))) => return Err(err),
                Err(err) => last_err = Some(err), // 5xx / 网络错误 → 重试
            }
        }
        Err(last_err.unwrap_or_else(|| FetchError::Network("no attempt made".into())))
    }

    /// 用户明确指定的单资源获取（[`Fetcher::fetch_direct`] 的覆写）：
    /// 跳过 robots——robots 管的是"爬虫发现式抓取"，用户点名的 URL
    /// （封面图 / 钉版字体）不属此类（github.com robots 禁 release
    /// 资产路径曾致 exact 字体下载被误拒，2026-10）。限速与重试语义
    /// 与 [`Fetcher::fetch`] 一致。
    fn fetch_direct_impl(
        &self,
        url: &Url,
        max_bytes: Option<u64>,
    ) -> Result<FetchedPage, FetchError> {
        Self::ensure_http_scheme(url)?;
        let limit = max_bytes.unwrap_or(self.config.max_bytes);
        self.get_with_retry(url, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_polite() {
        let cfg = PoliteConfig::default();
        assert!(cfg.respect_robots);
        assert_eq!(cfg.min_interval, Duration::from_secs(1));
        assert_eq!(cfg.retries, 2);
        assert!(cfg.user_agent.starts_with("trans-epub/"));
        assert!(cfg.user_agent.contains("personal use"));
    }

    #[test]
    fn robots_allowed_semantics() {
        let body = b"User-agent: *\nDisallow: /private/\n";
        let robot = texting_robots::Robot::new("trans-epub/x", body).expect("parses");
        assert!(!robot.allowed("/private/secret"));
        assert!(robot.allowed("/public"));
    }
}
