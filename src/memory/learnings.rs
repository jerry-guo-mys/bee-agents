//! 自我改进智能体：将学习内容、错误、功能需求记录到 .learnings/*.md，实现持续改进
//!
//! - 命令/操作失败 → .learnings/ERRORS.md
//! - 用户纠正 → .learnings/LEARNINGS.md (category: correction)
//! - 用户想要缺失的功能 → .learnings/FEATURE_REQUESTS.md
//! - API/外部工具失败 → .learnings/ERRORS.md（含集成详情）
//! - 知识过时 → .learnings/LEARNINGS.md (category: knowledge_gap)
//! - 发现更好的方法 → .learnings/LEARNINGS.md (category: best_practice)
//! - 与现有条目相似时可使用 **See Also** 链接，考虑提升优先级
//!
//! **提升到工作区**：当学习内容被证明广泛适用时，可提升到 workspace 根目录的：
//! - SOUL.md — 行为模式（简洁明了，避免免责声明）
//! - AGENTS.md — 工作流改进（如：长任务生成子代理）
//! - TOOLS.md — 工具技巧（如：Git push 需要先配置认证）

use std::io::Write;
use std::path::{Path, PathBuf};

/// .learnings 目录：位于 workspace 下
pub fn learnings_root(workspace: &Path) -> PathBuf {
    workspace.join(".learnings")
}

pub fn errors_path(workspace: &Path) -> PathBuf {
    learnings_root(workspace).join("ERRORS.md")
}

pub fn learnings_path(workspace: &Path) -> PathBuf {
    learnings_root(workspace).join("LEARNINGS.md")
}

pub fn feature_requests_path(workspace: &Path) -> PathBuf {
    learnings_root(workspace).join("FEATURE_REQUESTS.md")
}

// ---------- 提升到工作区：广泛适用的学习内容 ----------

/// 行为模式：workspace/SOUL.md（简洁明了，避免免责声明）
pub fn soul_path(workspace: &Path) -> PathBuf {
    workspace.join("SOUL.md")
}

/// 工作流改进：workspace/AGENTS.md（如：长任务生成子代理）
pub fn agents_path(workspace: &Path) -> PathBuf {
    workspace.join("AGENTS.md")
}

/// 工具技巧：workspace/TOOLS.md（如：Git push 需要先配置认证）
pub fn tools_guide_path(workspace: &Path) -> PathBuf {
    workspace.join("TOOLS.md")
}

fn ensure_dir(path: &Path) -> std::io::Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    Ok(())
}

fn timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

/// 确保文件存在且带标题（首次创建时写入）
fn ensure_header(path: &Path, title: &str) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    ensure_dir(path)?;
    let header = format!("# {}\n\n*Self-improving agent: entries appended below.*\n\n---\n\n", title);
    std::fs::write(path, header)
}

/// 工作区提升文件：首次创建时写入说明性标题
fn ensure_workspace_header(path: &Path, title: &str, subtitle: &str) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    ensure_dir(path)?;
    let header = format!(
        "# {}\n\n{}\n\n*当学习内容被证明广泛适用时，可追加到此文件。*\n\n---\n\n",
        title, subtitle
    );
    std::fs::write(path, header)
}

/// 命令/操作失败 或 API/外部工具失败 → ERRORS.md
pub fn record_error(workspace: &Path, tool: &str, reason: &str) {
    let path = errors_path(workspace);
    let _ = ensure_header(&path, "Errors");
    let block = format!(
        "\n## {} — {}\n\n- **Tool**: `{}`\n- **Reason**: {}\n\n",
        timestamp(),
        tool,
        tool,
        reason.trim().replace("\n", " ")
    );
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(block.as_bytes()));
}

/// 学习内容 → LEARNINGS.md，按分类：correction | knowledge_gap | best_practice
pub fn record_learning(workspace: &Path, category: &str, content: &str, see_also: Option<&str>) {
    let path = learnings_path(workspace);
    let _ = ensure_header(&path, "Learnings");
    let see = see_also
        .map(|s| format!("\n- **See Also**: {}", s.trim()))
        .unwrap_or_default();
    let block = format!(
        "\n## {} [{}]\n\n{}\n{}\n\n",
        timestamp(),
        category,
        content.trim(),
        see
    );
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(block.as_bytes()));
}

/// 用户想要缺失的功能 → FEATURE_REQUESTS.md
pub fn record_feature_request(workspace: &Path, description: &str) {
    let path = feature_requests_path(workspace);
    let _ = ensure_header(&path, "Feature Requests");
    let block = format!("\n- **{}**: {}\n\n", timestamp(), description.trim().replace("\n", " "));
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(block.as_bytes()));
}

// ---------- 提升到工作区：广泛适用的学习内容 ----------

/// 当学习内容被证明广泛适用时，提升到 SOUL.md（行为模式：简洁明了，避免免责声明）
pub fn promote_to_soul(workspace: &Path, content: &str) {
    let path = soul_path(workspace);
    let _ = ensure_workspace_header(
        &path,
        "SOUL",
        "行为模式：简洁明了，避免免责声明。",
    );
    let line = format!("- {}\n", content.trim().replace("\n", " "));
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// 当学习内容被证明广泛适用时，提升到 AGENTS.md（工作流改进：如长任务生成子代理）
pub fn promote_to_agents(workspace: &Path, content: &str) {
    let path = agents_path(workspace);
    let _ = ensure_workspace_header(
        &path,
        "AGENTS",
        "工作流改进：长任务生成子代理等。",
    );
    let line = format!("- {}\n", content.trim().replace("\n", " "));
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// 当学习内容被证明广泛适用时，提升到 TOOLS.md（工具技巧：如 Git push 需要先配置认证）
pub fn promote_to_tools(workspace: &Path, content: &str) {
    let path = tools_guide_path(workspace);
    let _ = ensure_workspace_header(
        &path,
        "TOOLS",
        "工具技巧：如 Git push 需要先配置认证。",
    );
    let line = format!("- {}\n", content.trim().replace("\n", " "));
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}
