//! Search/Web 工具：域名白名单、超时、结果大小限制
//!
//! 仅允许配置中的域名（如 wikipedia、docs.rs）；GET 请求带超时与 User-Agent；
//! 响应超过 max_result_chars 时截断并追加 ...[truncated]。
//! 对 HTML 响应使用 html2text 提取可读文本，去除标签与脚本。

use std::collections::HashSet;

use async_trait::async_trait;
use html2text::from_read;
use reqwest::Client;
use serde_json::Value;

use crate::tools::Tool;

/// Search 工具：抓取 URL 内容，仅允许白名单域名；超时与最大字符数由配置决定
pub struct SearchTool {
    client: Client,
    allowed_domains: HashSet<String>,
    max_result_chars: usize,
}

/// 简易去除 HTML 标签（html2text 失败时的回退）
fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut prev_whitespace = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => {
                let is_whitespace = c.is_whitespace();
                if is_whitespace && prev_whitespace {
                    continue;
                }
                prev_whitespace = is_whitespace;
                out.push(if is_whitespace { ' ' } else { c });
            }
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ").trim().to_string()
}

/// 判断内容是否像 HTML（需提取可读文本）
fn looks_like_html(s: &str) -> bool {
    let s = s.trim_start();
    s.starts_with("<!") || s.starts_with("<html") || s.starts_with("<HTML")
        || (s.len() > 20 && s.contains('<') && (s.contains("</") || s.contains("<meta") || s.contains("<head") || s.contains("<title")))
}

/// 从 URL 中提取 host（不含端口后的路径）
fn extract_domain(url: &str) -> Option<String> {
    let url = url.trim();
    let url = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = url.split('/').next()?;
    let host = host.split(':').next()?;
    Some(host.to_lowercase())
}

impl SearchTool {
    pub fn new(
        allowed_domains: Vec<String>,
        timeout_secs: u64,
        max_result_chars: usize,
    ) -> Self {
        let allowed_domains = allowed_domains
            .into_iter()
            .map(|s| s.to_lowercase())
            .collect();
        // 使用现代浏览器 UA 与常用请求头，避免被站点识别为低版本或爬虫
        const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .user_agent(USER_AGENT)
            .default_headers({
                use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE};
                let mut h = reqwest::header::HeaderMap::new();
                h.insert(ACCEPT, "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".parse().unwrap());
                h.insert(ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9,en;q=0.8".parse().unwrap());
                h
            })
            .build()
            .unwrap_or_default();
        Self {
            client,
            allowed_domains,
            max_result_chars,
        }
    }

    fn is_allowed(&self, url: &str) -> Result<(), String> {
        let domain = extract_domain(url)
            .ok_or_else(|| "Invalid or missing URL".to_string())?;
        if self.allowed_domains.contains(&domain) {
            return Ok(());
        }
        Err(format!("Domain not in allowlist: {}", domain))
    }

    /// 将 HTML 转为可读文本（去除 script/style 等）
    fn html_to_text(&self, html: &str) -> String {
        match from_read(html.as_bytes(), 120) {
            Ok(text) if !text.trim().is_empty() => text,
            _ => strip_html_tags(html),
        }
    }

    async fn fetch(&self, url: &str) -> Result<String, String> {
        self.is_allowed(url)?;
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let mut body = resp
            .text()
            .await
            .map_err(|e| format!("Read body: {}", e))?;

        // 去除 BOM，避免 HTML 检测失败
        if body.starts_with('\u{FEFF}') {
            body = body[1..].to_string();
        }

        // 若为 HTML，提取可读文本（GitHub、维基等均返回 HTML）
        let body = if looks_like_html(&body) {
            self.html_to_text(&body)
        } else {
            body
        };

        let len = body.chars().count();
        if len > self.max_result_chars {
            Ok(body.chars().take(self.max_result_chars).collect::<String>()
                + "\n...[truncated]")
        } else {
            Ok(body)
        }
    }
}

#[async_trait]
impl Tool for SearchTool {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Fetch URL content (domain allowlist: Wikipedia, Baidu, JD, Zhihu, GitHub, StackOverflow, docs.rs, MDN, arxiv, etc). Args: {\"url\": \"https://...\"}."
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if url.is_empty() {
            return Err("Missing url".to_string());
        }
        tracing::info!(url = %url, "search tool fetch");
        self.fetch(url).await
    }
}
