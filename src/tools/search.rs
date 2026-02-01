//! Search/Web 工具：域名白名单、超时、结果大小限制
//!
//! 仅允许配置中的域名（如 wikipedia、docs.rs）；GET 请求带超时与 User-Agent；
//! 响应超过 max_result_chars 时截断并追加 ...[truncated]。

use std::collections::HashSet;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::tools::Tool;

/// Search 工具：抓取 URL 内容，仅允许白名单域名；超时与最大字符数由配置决定
pub struct SearchTool {
    client: Client,
    allowed_domains: HashSet<String>,
    max_result_chars: usize,
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
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .user_agent("Bee-Agent/1.0")
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
        let body = resp
            .text()
            .await
            .map_err(|e| format!("Read body: {}", e))?;
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
