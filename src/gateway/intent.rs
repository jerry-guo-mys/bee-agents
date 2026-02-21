//! 意图识别模块
//!
//! 分析用户输入，识别意图并路由到合适的能力端点

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::llm::LlmClient;
use crate::memory::Message;

/// 识别出的意图类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    /// 普通对话（聊天、问答）
    Chat,
    /// 代码相关（编写、审查、调试）
    Code {
        action: CodeAction,
    },
    /// 搜索信息
    Search {
        query: String,
    },
    /// 文件操作
    FileOperation {
        action: FileAction,
        path: Option<String>,
    },
    /// 系统命令执行
    Shell {
        command: Option<String>,
    },
    /// 使用特定技能
    UseSkill {
        skill_id: String,
    },
    /// 记忆相关（回忆、总结）
    Memory {
        action: MemoryAction,
    },
    /// 任务管理（创建、查看、完成）
    Task {
        action: TaskAction,
    },
    /// 浏览网页
    Browse {
        url: Option<String>,
    },
    /// 不确定，需要进一步澄清
    Unclear,
}

/// 代码操作类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeAction {
    Write,
    Edit,
    Review,
    Debug,
    Explain,
    Test,
}

/// 文件操作类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileAction {
    Read,
    Write,
    List,
    Search,
    Delete,
}

/// 记忆操作类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAction {
    Recall,
    Summarize,
    Forget,
}

/// 任务操作类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskAction {
    Create,
    List,
    Complete,
    Cancel,
}

/// 意图识别器
pub struct IntentRecognizer {
    llm: Arc<dyn LlmClient>,
    /// 启用快速规则匹配（不调用 LLM）
    enable_fast_match: bool,
}

impl IntentRecognizer {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self {
            llm,
            enable_fast_match: true,
        }
    }

    /// 识别用户意图
    pub async fn recognize(&self, user_input: &str) -> Intent {
        if self.enable_fast_match {
            if let Some(intent) = self.fast_match(user_input) {
                return intent;
            }
        }

        self.llm_recognize(user_input).await.unwrap_or(Intent::Chat)
    }

    /// 快速规则匹配（不调用 LLM）
    fn fast_match(&self, input: &str) -> Option<Intent> {
        let input_lower = input.to_lowercase();
        let input_trimmed = input.trim();

        if input_lower.starts_with("搜索")
            || input_lower.starts_with("search")
            || input_lower.starts_with("查一下")
            || input_lower.starts_with("帮我查")
        {
            let query = input_trimmed
                .trim_start_matches("搜索")
                .trim_start_matches("search")
                .trim_start_matches("查一下")
                .trim_start_matches("帮我查")
                .trim()
                .to_string();
            return Some(Intent::Search { query });
        }

        if input_lower.starts_with("打开")
            || input_lower.starts_with("访问")
            || input_lower.starts_with("browse")
            || input_lower.contains("http://")
            || input_lower.contains("https://")
        {
            let url = extract_url(input);
            return Some(Intent::Browse { url });
        }

        if input_lower.starts_with("运行")
            || input_lower.starts_with("执行")
            || input_lower.starts_with("run")
            || input_lower.starts_with("exec")
            || input_lower.starts_with("$")
        {
            let command = input_trimmed
                .trim_start_matches("运行")
                .trim_start_matches("执行")
                .trim_start_matches("run")
                .trim_start_matches("exec")
                .trim_start_matches("$")
                .trim();
            return Some(Intent::Shell {
                command: if command.is_empty() {
                    None
                } else {
                    Some(command.to_string())
                },
            });
        }

        if input_lower.starts_with("读取")
            || input_lower.starts_with("查看文件")
            || input_lower.starts_with("cat ")
            || input_lower.starts_with("read ")
        {
            return Some(Intent::FileOperation {
                action: FileAction::Read,
                path: None,
            });
        }

        if input_lower.starts_with("列出")
            || input_lower.starts_with("ls ")
            || input_lower.starts_with("list ")
        {
            return Some(Intent::FileOperation {
                action: FileAction::List,
                path: None,
            });
        }

        if input_lower.contains("写代码")
            || input_lower.contains("编写")
            || input_lower.contains("实现")
            || input_lower.starts_with("write code")
            || input_lower.starts_with("implement")
        {
            return Some(Intent::Code {
                action: CodeAction::Write,
            });
        }

        if input_lower.contains("审查代码")
            || input_lower.contains("review")
            || input_lower.contains("检查代码")
        {
            return Some(Intent::Code {
                action: CodeAction::Review,
            });
        }

        if input_lower.contains("调试")
            || input_lower.contains("debug")
            || input_lower.contains("找bug")
            || input_lower.contains("修复")
        {
            return Some(Intent::Code {
                action: CodeAction::Debug,
            });
        }

        if input_lower.starts_with("回忆")
            || input_lower.starts_with("记得")
            || input_lower.contains("之前说过")
            || input_lower.contains("上次")
        {
            return Some(Intent::Memory {
                action: MemoryAction::Recall,
            });
        }

        if input_lower.starts_with("总结")
            || input_lower.starts_with("summarize")
            || input_lower.contains("概括")
        {
            return Some(Intent::Memory {
                action: MemoryAction::Summarize,
            });
        }

        if input_lower.starts_with("创建任务")
            || input_lower.starts_with("新建任务")
            || input_lower.starts_with("todo:")
        {
            return Some(Intent::Task {
                action: TaskAction::Create,
            });
        }

        if input_lower.starts_with("任务列表")
            || input_lower.starts_with("查看任务")
            || input_lower.starts_with("todos")
        {
            return Some(Intent::Task {
                action: TaskAction::List,
            });
        }

        None
    }

    /// 使用 LLM 识别意图
    async fn llm_recognize(&self, user_input: &str) -> Result<Intent, String> {
        let system_prompt = r#"You are an intent classifier. Analyze the user's input and classify their intent.

Output ONLY one of these intent types (no explanation):
- chat: General conversation, questions, or discussion
- code_write: Writing new code
- code_edit: Modifying existing code
- code_review: Reviewing code quality
- code_debug: Finding and fixing bugs
- search: Looking for information online
- file_read: Reading file contents
- file_write: Creating or modifying files
- file_list: Listing directory contents
- shell: Running system commands
- memory_recall: Remembering past conversations
- memory_summarize: Summarizing information
- task_create: Creating a new task
- task_list: Viewing tasks
- browse: Visiting a webpage
- unclear: Cannot determine intent

Output format: just the intent type, nothing else."#;

        let messages = vec![
            Message::system(system_prompt.to_string()),
            Message::user(format!("User input: {}", user_input)),
        ];

        let response = self
            .llm
            .complete(&messages)
            .await
            .map_err(|e| e.to_string())?;

        let intent_str = response.trim().to_lowercase();

        Ok(match intent_str.as_str() {
            "chat" => Intent::Chat,
            "code_write" => Intent::Code {
                action: CodeAction::Write,
            },
            "code_edit" => Intent::Code {
                action: CodeAction::Edit,
            },
            "code_review" => Intent::Code {
                action: CodeAction::Review,
            },
            "code_debug" => Intent::Code {
                action: CodeAction::Debug,
            },
            "search" => Intent::Search {
                query: user_input.to_string(),
            },
            "file_read" => Intent::FileOperation {
                action: FileAction::Read,
                path: None,
            },
            "file_write" => Intent::FileOperation {
                action: FileAction::Write,
                path: None,
            },
            "file_list" => Intent::FileOperation {
                action: FileAction::List,
                path: None,
            },
            "shell" => Intent::Shell { command: None },
            "memory_recall" => Intent::Memory {
                action: MemoryAction::Recall,
            },
            "memory_summarize" => Intent::Memory {
                action: MemoryAction::Summarize,
            },
            "task_create" => Intent::Task {
                action: TaskAction::Create,
            },
            "task_list" => Intent::Task {
                action: TaskAction::List,
            },
            "browse" => Intent::Browse {
                url: extract_url(user_input),
            },
            "unclear" => Intent::Unclear,
            _ => Intent::Chat,
        })
    }

    /// 根据意图推荐使用的工具
    pub fn suggest_tools(&self, intent: &Intent) -> Vec<String> {
        match intent {
            Intent::Chat => vec![],
            Intent::Code { action } => match action {
                CodeAction::Write => vec!["code_write".to_string()],
                CodeAction::Edit => vec!["code_edit".to_string()],
                CodeAction::Review => vec!["code_read".to_string(), "code_grep".to_string()],
                CodeAction::Debug => vec![
                    "code_read".to_string(),
                    "code_grep".to_string(),
                    "test_run".to_string(),
                ],
                CodeAction::Explain => vec!["code_read".to_string()],
                CodeAction::Test => vec!["test_run".to_string(), "test_check".to_string()],
            },
            Intent::Search { .. } => vec!["search".to_string(), "deep_search".to_string()],
            Intent::FileOperation { action, .. } => match action {
                FileAction::Read => vec!["cat".to_string(), "code_read".to_string()],
                FileAction::Write => vec!["code_write".to_string()],
                FileAction::List => vec!["ls".to_string()],
                FileAction::Search => vec!["code_grep".to_string()],
                FileAction::Delete => vec!["shell".to_string()],
            },
            Intent::Shell { .. } => vec!["shell".to_string()],
            Intent::UseSkill { .. } => vec![],
            Intent::Memory { .. } => vec![],
            Intent::Task { .. } => vec![],
            Intent::Browse { .. } => vec!["browser".to_string()],
            Intent::Unclear => vec![],
        }
    }

    /// 根据意图推荐使用的技能
    pub fn suggest_skills(&self, intent: &Intent) -> Vec<String> {
        match intent {
            Intent::Code { action } => match action {
                CodeAction::Write | CodeAction::Edit => vec!["writing".to_string()],
                CodeAction::Review => vec!["claude".to_string()],
                _ => vec![],
            },
            Intent::Search { .. } => vec!["search".to_string()],
            Intent::UseSkill { skill_id } => vec![skill_id.clone()],
            _ => vec![],
        }
    }
}

/// 从文本中提取 URL
fn extract_url(text: &str) -> Option<String> {
    for word in text.split_whitespace() {
        if word.starts_with("http://") || word.starts_with("https://") {
            return Some(word.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fast_match_search() {
        let recognizer = IntentRecognizer {
            llm: Arc::new(crate::llm::MockLlmClient),
            enable_fast_match: true,
        };

        let intent = recognizer.fast_match("搜索 Rust 异步编程");
        assert!(matches!(intent, Some(Intent::Search { query }) if query.contains("Rust")));
    }

    #[test]
    fn test_fast_match_browse() {
        let recognizer = IntentRecognizer {
            llm: Arc::new(crate::llm::MockLlmClient),
            enable_fast_match: true,
        };

        let intent = recognizer.fast_match("打开 https://example.com");
        assert!(matches!(intent, Some(Intent::Browse { url: Some(_) })));
    }

    #[test]
    fn test_fast_match_shell() {
        let recognizer = IntentRecognizer {
            llm: Arc::new(crate::llm::MockLlmClient),
            enable_fast_match: true,
        };

        let intent = recognizer.fast_match("运行 cargo test");
        assert!(matches!(intent, Some(Intent::Shell { command: Some(_) })));
    }
}
