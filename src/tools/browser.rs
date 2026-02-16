//! Browser 工具：使用 Headless Chrome 控制浏览器提取内容
//!
//! 需启用 feature "browser" 且系统已安装 Chrome/Chromium。
//! 访问 URL、执行 JS 渲染后提取可读文本（适用于 Search 无法处理的动态页面）。

use std::collections::HashSet;

use async_trait::async_trait;
use headless_chrome::Browser;
use serde_json::Value;

use crate::tools::Tool;

/// 从 URL 提取域名（小写）
fn extract_domain(url: &str) -> Option<String> {
    let url = url.trim();
    let url = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = url.split('/').next()?;
    let host = host.split(':').next()?;
    Some(host.to_lowercase())
}

/// Browser 工具：Headless Chrome 访问 URL、提取页面可读文本
pub struct BrowserTool {
    allowed_domains: HashSet<String>,
    max_result_chars: usize,
}

impl BrowserTool {
    pub fn new(allowed_domains: Vec<String>, max_result_chars: usize) -> Self {
        let allowed_domains = allowed_domains
            .into_iter()
            .map(|s| s.to_lowercase())
            .collect();
        Self {
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
}

#[async_trait]
impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Use headless browser to visit URL and extract rendered page content (requires Chrome). Use for JS-heavy pages. Args: {\"url\": \"https://...\", \"selector\": \"optional CSS selector\"}. Domain allowlist same as search."
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
        self.is_allowed(url)?;

        let selector = args.get("selector").and_then(|v| v.as_str()).map(|s| s.to_string());
        let max_chars = self.max_result_chars;
        let url = url.to_string();

        tracing::info!(url = %url, selector = ?selector, "browser tool fetch");

        let text = tokio::task::spawn_blocking(move || {
            let browser = Browser::default()
                .map_err(|e| format!("Chrome launch failed: {}. Install Chrome/Chromium.", e))?;
            let tab = browser
                .new_tab()
                .map_err(|e| format!("Browser tab failed: {}", e))?;
            tab.navigate_to(&url)
                .map_err(|e| format!("Navigate failed: {}", e))?;
            tab.wait_for_element("body")
                .map_err(|e| format!("Page load failed: {}", e))?;

            let text = if let Some(sel) = selector {
                let el = tab
                    .wait_for_element(&sel)
                    .map_err(|e| format!("Element not found: {}", e))?;
                el.get_inner_text()
                    .map_err(|e| format!("Get text failed: {}", e))?
            } else {
                let content = tab
                    .get_content()
                    .map_err(|e| format!("Get content failed: {}", e))?;
                html2text::from_read(content.as_bytes(), 120).unwrap_or_else(|_| content)
            };

            let len = text.chars().count();
            if len > max_chars {
                Ok::<_, String>(text.chars().take(max_chars).collect::<String>() + "\n...[truncated]")
            } else {
                Ok(text)
            }
        })
        .await
        .map_err(|e| format!("Task join: {}", e))??;

        Ok(text)
    }
}
