//! Browser 工具：使用 Headless Chrome 控制浏览器提取内容
//!
//! 需启用 feature "browser" 且系统已安装 Chrome/Chromium。
//! 访问 URL、执行 JS 渲染后提取可读文本（适用于 Search 无法处理的动态页面）。
//!
//! ## 语义快照（Semantic Snapshot）
//!
//! 通过抓取网页的无障碍树（Accessibility Tree），将其转化为高度结构化的语义文本。
//! - 降低 Token 开销（相比完整 HTML）
//! - AI 能够精准定位并点击特定的 DOM 节点
//! - 每个可交互元素都有唯一的引用 ID（如 [1], [2]）

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use headless_chrome::{Browser, Tab};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::Tool;

/// 语义快照中的元素
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticElement {
    pub ref_id: usize,
    pub role: String,
    pub name: String,
    pub value: Option<String>,
    pub description: Option<String>,
    pub backend_node_id: Option<i64>,
    pub is_interactive: bool,
    pub depth: usize,
}

/// 语义快照结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSnapshot {
    pub url: String,
    pub title: String,
    pub elements: Vec<SemanticElement>,
    pub text_representation: String,
}

/// 浏览器会话状态（用于持久化 Tab 和元素映射）
pub struct BrowserSession {
    pub tab: Arc<Tab>,
    pub element_map: HashMap<usize, i64>,
    pub current_url: String,
}

/// 从 URL 提取域名（小写）
fn extract_domain(url: &str) -> Option<String> {
    let url = url.trim();
    let url = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = url.split('/').next()?;
    let host = host.split(':').next()?;
    Some(host.to_lowercase())
}

/// 判断角色是否为可交互元素
fn is_interactive_role(role: &str) -> bool {
    matches!(
        role.to_lowercase().as_str(),
        "button"
            | "link"
            | "textbox"
            | "checkbox"
            | "radio"
            | "combobox"
            | "listbox"
            | "menuitem"
            | "menuitemcheckbox"
            | "menuitemradio"
            | "option"
            | "searchbox"
            | "slider"
            | "spinbutton"
            | "switch"
            | "tab"
            | "treeitem"
    )
}

/// 构建语义快照的文本表示
fn build_text_representation(elements: &[SemanticElement]) -> String {
    let mut lines = Vec::new();
    
    for elem in elements {
        let indent = "  ".repeat(elem.depth);
        let ref_marker = if elem.is_interactive {
            format!("[{}] ", elem.ref_id)
        } else {
            String::new()
        };
        
        let mut line = format!("{}{}{}", indent, ref_marker, elem.role);
        
        if !elem.name.is_empty() {
            line.push_str(&format!(": \"{}\"", elem.name));
        }
        
        if let Some(ref val) = elem.value {
            line.push_str(&format!(" = \"{}\"", val));
        }
        
        if let Some(ref desc) = elem.description {
            line.push_str(&format!(" ({})", desc));
        }
        
        lines.push(line);
    }
    
    lines.join("\n")
}

/// Browser 工具：Headless Chrome 访问 URL、提取页面可读文本
///
/// 支持两种模式：
/// - 传统模式：提取页面文本内容
/// - 语义快照模式：获取无障碍树，返回结构化语义文本
pub struct BrowserTool {
    allowed_domains: HashSet<String>,
    max_result_chars: usize,
    session: Arc<RwLock<Option<BrowserSession>>>,
    browser: Arc<RwLock<Option<Browser>>>,
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
            session: Arc::new(RwLock::new(None)),
            browser: Arc::new(RwLock::new(None)),
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

    /// 获取语义快照
    pub fn get_semantic_snapshot(tab: &Arc<Tab>) -> Result<SemanticSnapshot, String> {
        let url = tab.get_url();
        let title = tab
            .get_title()
            .map_err(|e| format!("Get title failed: {}", e))?;

        let ax_tree = tab
            .call_method(headless_chrome::protocol::cdp::Accessibility::GetFullAXTree {
                depth: Some(10),
                frame_id: None,
            })
            .map_err(|e| format!("Get accessibility tree failed: {}", e))?;

        let mut elements = Vec::new();
        let mut element_map = HashMap::new();
        let mut ref_id = 1usize;

        for node in &ax_tree.nodes {
                let role = node.role.as_ref()
                    .and_then(|r| r.value.as_ref())
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                if role == "none" || role == "unknown" || role == "generic" {
                    continue;
                }

                let name = node.name.as_ref()
                    .and_then(|n| n.value.as_ref())
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let value = node.value.as_ref()
                    .and_then(|v| v.value.as_ref())
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let description = node.description.as_ref()
                    .and_then(|d| d.value.as_ref())
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let backend_node_id = node.backend_dom_node_id.map(|id| id as i64);
                let is_interactive = is_interactive_role(&role);

                let depth = 0;

                if is_interactive {
                    if let Some(id) = backend_node_id {
                        element_map.insert(ref_id, id);
                    }
                }

                elements.push(SemanticElement {
                    ref_id: if is_interactive { ref_id } else { 0 },
                    role,
                    name,
                    value,
                    description,
                    backend_node_id,
                    is_interactive,
                    depth,
                });

                if is_interactive {
                    ref_id += 1;
                }
        }

        let text_representation = build_text_representation(&elements);

        Ok(SemanticSnapshot {
            url,
            title,
            elements,
            text_representation,
        })
    }

    /// 通过引用 ID 点击元素（使用 JavaScript 执行）
    pub fn click_by_ref(tab: &Arc<Tab>, ref_id: usize, element_map: &HashMap<usize, i64>) -> Result<String, String> {
        let _backend_node_id = element_map
            .get(&ref_id)
            .ok_or_else(|| format!("Element ref [{}] not found", ref_id))?;

        let js = format!(
            r#"
            (function() {{
                // 通过 CDP 获取 DOM 节点
                return new Promise((resolve, reject) => {{
                    // 使用 TreeWalker 遍历所有可交互元素，找到第 {} 个
                    const interactiveRoles = ['button', 'link', 'textbox', 'checkbox', 'radio', 'combobox', 
                        'listbox', 'menuitem', 'option', 'searchbox', 'slider', 'spinbutton', 'switch', 'tab', 'treeitem'];
                    
                    const allElements = document.querySelectorAll('button, a, input, select, textarea, [role]');
                    let count = 0;
                    for (const el of allElements) {{
                        const role = el.getAttribute('role') || el.tagName.toLowerCase();
                        const isInteractive = interactiveRoles.includes(role) || 
                            ['button', 'a', 'input', 'select', 'textarea'].includes(el.tagName.toLowerCase());
                        if (isInteractive) {{
                            count++;
                            if (count === {}) {{
                                el.scrollIntoView({{ behavior: 'instant', block: 'center' }});
                                el.click();
                                resolve('clicked: ' + (el.textContent || el.value || el.tagName).substring(0, 50));
                                return;
                            }}
                        }}
                    }}
                    reject('Element not found');
                }});
            }})()
            "#,
            ref_id, ref_id
        );

        let result = tab
            .evaluate(&js, true)
            .map_err(|e| format!("Click failed: {}", e))?;

        if let Some(val) = result.value {
            Ok(format!("Clicked element [{}]: {}", ref_id, val))
        } else {
            Ok(format!("Clicked element [{}]", ref_id))
        }
    }

    /// 在元素中输入文本（使用 JavaScript 执行）
    pub fn type_text_by_ref(
        tab: &Arc<Tab>,
        ref_id: usize,
        text: &str,
        element_map: &HashMap<usize, i64>,
    ) -> Result<String, String> {
        let _backend_node_id = element_map
            .get(&ref_id)
            .ok_or_else(|| format!("Element ref [{}] not found", ref_id))?;

        let escaped_text = text.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");

        let js = format!(
            r#"
            (function() {{
                const interactiveRoles = ['textbox', 'searchbox', 'combobox', 'spinbutton'];
                const allElements = document.querySelectorAll('input, textarea, [role="textbox"], [role="searchbox"], [contenteditable="true"]');
                let count = 0;
                for (const el of allElements) {{
                    count++;
                    if (count === {}) {{
                        el.scrollIntoView({{ behavior: 'instant', block: 'center' }});
                        el.focus();
                        if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA') {{
                            el.value = "{}";
                            el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                            el.dispatchEvent(new Event('change', {{ bubbles: true }}));
                        }} else {{
                            el.textContent = "{}";
                            el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                        }}
                        return 'typed';
                    }}
                }}
                return 'element not found';
            }})()
            "#,
            ref_id, escaped_text, escaped_text
        );

        tab.evaluate(&js, false)
            .map_err(|e| format!("Type failed: {}", e))?;

        Ok(format!("Typed \"{}\" into element [{}]", text, ref_id))
    }
}

#[async_trait]
impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        r#"Headless browser with semantic snapshots for precise interaction.

Actions:
- navigate: Visit URL and get semantic snapshot
  Args: {"action": "navigate", "url": "https://..."}
  Returns: Structured accessibility tree with interactive elements marked as [1], [2], etc.

- snapshot: Get current page semantic snapshot (refresh element refs)
  Args: {"action": "snapshot"}

- click: Click element by reference ID
  Args: {"action": "click", "ref": 1}

- type: Type text into element
  Args: {"action": "type", "ref": 1, "text": "hello"}

- scroll: Scroll page
  Args: {"action": "scroll", "direction": "down"} (or "up")

- content: Get page text content (legacy mode)
  Args: {"action": "content", "url": "...", "selector": "optional CSS"}

The semantic snapshot shows interactive elements like:
  [1] button: "Submit"
  [2] textbox: "Search"
  [3] link: "About Us"
Use ref IDs to interact with elements precisely."#
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("content");

        match action {
            "navigate" => {
                let url = args
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if url.is_empty() {
                    return Err("Missing url".to_string());
                }
                self.is_allowed(url)?;

                let url = url.to_string();
                let session_arc = Arc::clone(&self.session);
                let browser_arc = Arc::clone(&self.browser);
                let max_chars = self.max_result_chars;

                tracing::info!(url = %url, "browser navigate with semantic snapshot");

                let result = tokio::task::spawn_blocking(move || {
                    let mut browser_guard = browser_arc.write().map_err(|e| e.to_string())?;
                    if browser_guard.is_none() {
                        let browser = Browser::default()
                            .map_err(|e| format!("Chrome launch failed: {}", e))?;
                        *browser_guard = Some(browser);
                    }
                    let browser = browser_guard.as_ref().unwrap();

                    let tab = browser
                        .new_tab()
                        .map_err(|e| format!("Browser tab failed: {}", e))?;
                    tab.navigate_to(&url)
                        .map_err(|e| format!("Navigate failed: {}", e))?;
                    tab.wait_for_element("body")
                        .map_err(|e| format!("Page load failed: {}", e))?;

                    std::thread::sleep(std::time::Duration::from_millis(500));

                    let snapshot = Self::get_semantic_snapshot(&tab)?;

                    let mut element_map = HashMap::new();
                    for elem in &snapshot.elements {
                        if elem.is_interactive {
                            if let Some(id) = elem.backend_node_id {
                                element_map.insert(elem.ref_id, id);
                            }
                        }
                    }

                    let mut session_guard = session_arc.write().map_err(|e| e.to_string())?;
                    *session_guard = Some(BrowserSession {
                        tab,
                        element_map,
                        current_url: url,
                    });

                    let output = format!(
                        "# {}\nURL: {}\n\n## Semantic Snapshot\n{}",
                        snapshot.title,
                        snapshot.url,
                        snapshot.text_representation
                    );

                    if output.len() > max_chars {
                        Ok::<_, String>(output.chars().take(max_chars).collect::<String>() + "\n...[truncated]")
                    } else {
                        Ok(output)
                    }
                })
                .await
                .map_err(|e| format!("Task join: {}", e))??;

                Ok(result)
            }

            "snapshot" => {
                let session_arc = Arc::clone(&self.session);
                let max_chars = self.max_result_chars;

                let result = tokio::task::spawn_blocking(move || {
                    let session_guard = session_arc.read().map_err(|e| e.to_string())?;
                    let session = session_guard
                        .as_ref()
                        .ok_or_else(|| "No active browser session. Use navigate first.".to_string())?;

                    let snapshot = Self::get_semantic_snapshot(&session.tab)?;

                    let output = format!(
                        "# {}\nURL: {}\n\n## Semantic Snapshot\n{}",
                        snapshot.title,
                        snapshot.url,
                        snapshot.text_representation
                    );

                    if output.len() > max_chars {
                        Ok::<_, String>(output.chars().take(max_chars).collect::<String>() + "\n...[truncated]")
                    } else {
                        Ok(output)
                    }
                })
                .await
                .map_err(|e| format!("Task join: {}", e))??;

                Ok(result)
            }

            "click" => {
                let ref_id = args
                    .get("ref")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| "Missing ref (element reference ID)".to_string())?
                    as usize;

                let session_arc = Arc::clone(&self.session);

                let result = tokio::task::spawn_blocking(move || {
                    let session_guard = session_arc.read().map_err(|e| e.to_string())?;
                    let session = session_guard
                        .as_ref()
                        .ok_or_else(|| "No active browser session. Use navigate first.".to_string())?;

                    Self::click_by_ref(&session.tab, ref_id, &session.element_map)
                })
                .await
                .map_err(|e| format!("Task join: {}", e))??;

                Ok(result)
            }

            "type" => {
                let ref_id = args
                    .get("ref")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| "Missing ref (element reference ID)".to_string())?
                    as usize;
                let text = args
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let session_arc = Arc::clone(&self.session);
                let text = text.to_string();

                let result = tokio::task::spawn_blocking(move || {
                    let session_guard = session_arc.read().map_err(|e| e.to_string())?;
                    let session = session_guard
                        .as_ref()
                        .ok_or_else(|| "No active browser session. Use navigate first.".to_string())?;

                    Self::type_text_by_ref(&session.tab, ref_id, &text, &session.element_map)
                })
                .await
                .map_err(|e| format!("Task join: {}", e))??;

                Ok(result)
            }

            "scroll" => {
                let direction = args
                    .get("direction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("down");

                let session_arc = Arc::clone(&self.session);
                let direction = direction.to_string();

                let result = tokio::task::spawn_blocking(move || {
                    let session_guard = session_arc.read().map_err(|e| e.to_string())?;
                    let session = session_guard
                        .as_ref()
                        .ok_or_else(|| "No active browser session. Use navigate first.".to_string())?;

                    let scroll_amount = if direction == "up" { -500 } else { 500 };
                    let js = format!("window.scrollBy(0, {})", scroll_amount);
                    session.tab
                        .evaluate(&js, false)
                        .map_err(|e| format!("Scroll failed: {}", e))?;

                    Ok::<_, String>(format!("Scrolled {}", direction))
                })
                .await
                .map_err(|e| format!("Task join: {}", e))??;

                Ok(result)
            }

            "content" | _ => {
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

                tracing::info!(url = %url, selector = ?selector, "browser tool fetch content");

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
    }
}
