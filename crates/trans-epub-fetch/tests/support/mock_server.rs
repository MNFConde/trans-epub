//! 测试用最小 HTTP 服务：绑定 127.0.0.1 随机端口，按路径应答固定路由，
//! 记录命中次数与请求时刻（限速节拍断言）。全程不出回环地址。
//!（40-fetch.md 测试策略：本地 mock HTTP 服务，不依赖外网。）

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct Route {
    status: u16,
    content_type: String,
    body: Vec<u8>,
    /// 前 N 次命中改回 500（重试语义测试）。
    fail_first: usize,
    hits: Arc<AtomicUsize>,
}

#[derive(Clone, Default)]
pub struct MockServer {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    routes: Mutex<HashMap<String, Route>>,
    log: Mutex<Vec<(String, Instant)>>,
}

impl MockServer {
    pub fn new() -> Self {
        Self::default()
    }

    /// 启动监听（后台线程，每连接一线程），返回 base URL。`self` 可继续
    /// 用于 `hits` / `request_log` 断言（内部共享状态）。
    pub fn serve(&self) -> url::Url {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let server = self.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let server = server.clone();
                std::thread::spawn(move || server.handle(stream));
            }
        });
        format!("http://{addr}").parse().expect("base url")
    }

    /// 注册路由；`path` 含前导 `/`（可带 query）。借用以支持链式注册。
    pub fn route(
        &self,
        path: &str,
        status: u16,
        content_type: &str,
        body: impl Into<Vec<u8>>,
    ) -> &Self {
        let path = path.to_string();
        self.inner.routes.lock().unwrap().insert(
            path,
            Route {
                status,
                content_type: content_type.to_string(),
                body: body.into(),
                fail_first: 0,
                hits: Arc::new(AtomicUsize::new(0)),
            },
        );
        self
    }

    /// 前 `n` 次命中返回 500，之后按路由应答（重试测试）。
    pub fn route_fail_first_n(&self, path: &str, n: usize, body: &str) -> &Self {
        let route = Route {
            status: 200,
            content_type: "text/html".to_string(),
            body: body.as_bytes().to_vec(),
            fail_first: n,
            hits: Arc::new(AtomicUsize::new(0)),
        };
        self.inner
            .routes
            .lock()
            .unwrap()
            .insert(path.to_string(), route);
        self
    }

    pub fn hits(&self, path: &str) -> usize {
        self.inner
            .routes
            .lock()
            .unwrap()
            .get(path)
            .map(|r| r.hits.load(Ordering::SeqCst))
            .unwrap_or(0)
    }

    /// 请求日志（path + 时刻），限速节拍断言用。
    pub fn request_log(&self) -> Vec<(String, Instant)> {
        self.inner.log.lock().unwrap().clone()
    }

    fn handle(&self, mut stream: TcpStream) {
        let Some((path, _version)) = read_request(&mut stream) else {
            return;
        };
        self.inner
            .log
            .lock()
            .unwrap()
            .push((path.clone(), Instant::now()));
        let routes = self.inner.routes.lock().unwrap();
        let Some(route) = routes.get(&path) else {
            drop(routes);
            write_response(&mut stream, 404, "text/plain", b"not found");
            return;
        };
        let hit = route.hits.fetch_add(1, Ordering::SeqCst);
        if hit < route.fail_first {
            let (status, ct, body) = (500u16, "text/plain".to_string(), b"boom".to_vec());
            drop(routes);
            write_response(&mut stream, status, &ct, &body);
            return;
        }
        let (status, ct, body) = (route.status, route.content_type.clone(), route.body.clone());
        drop(routes);
        write_response(&mut stream, status, &ct, &body);
    }
}

/// 读请求头，返回 (path 含 query, HTTP 版本)。
fn read_request(stream: &mut TcpStream) -> Option<(String, &'static str)> {
    let mut buf = [0u8; 8192];
    let mut head = Vec::new();
    loop {
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        head.extend_from_slice(&buf[..n]);
        if head.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if head.len() > 64 * 1024 {
            return None;
        }
    }
    let text = String::from_utf8_lossy(&head);
    let mut parts = text.lines().next()?.split_whitespace();
    let _method = parts.next()?;
    let path = parts.next()?.to_string();
    let version = match parts.next()? {
        "HTTP/1.0" => "HTTP/1.0",
        _ => "HTTP/1.1",
    };
    Some((path, version))
}

fn write_response(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Status",
    };
    let head = format!(
        "{version} {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
        version = "HTTP/1.1",
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}
