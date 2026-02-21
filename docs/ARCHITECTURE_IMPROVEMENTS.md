# Bee 架构改进评估报告

> 基于 2026-02-21 对 `src/` 全源码的逐模块审查，按严重程度分级。
> 标注：🔴 严重 / 🟠 重要 / 🟡 建议 / ⚪ 长期演进

---

## 目录

- [一、核心架构问题](#一核心架构问题)
- [二、异步与并发问题](#二异步与并发问题)
- [三、LLM 层问题](#三llm-层问题)
- [四、ReAct 循环问题](#四react-循环问题)
- [五、记忆系统问题](#五记忆系统问题)
- [六、工具系统问题](#六工具系统问题)
- [七、可观测性与运维](#七可观测性与运维)
- [八、测试覆盖](#八测试覆盖)
- [九、代码质量](#九代码质量)
- [十、改进优先级路线图](#十改进优先级路线图)

---

## 一、核心架构问题

### 1.1 🔴 Agent 初始化逻辑重复且不一致

**位置**: `src/core/orchestrator.rs` vs `src/agent.rs`

**现状**:
- `create_agent()` (TUI 用) 只注册 5 个基础工具: cat, ls, echo, shell, search
- `create_agent_components()` (Web/WhatsApp 用) 注册 17+ 工具: 额外包含 code_read, code_grep, code_edit, code_write, test_run, test_check, git_commit, deep_search, source_validator, report_generator, knowledge_graph
- 两处工具注册代码大量重复，且 TUI 侧缺少大量能力

**影响**: TUI 用户无法使用代码编辑、深度搜索等核心工具；新增工具需改两处代码。

**建议**:
```rust
// 提取为统一的 AgentBuilder
pub struct AgentBuilder {
    config: AppConfig,
    workspace: PathBuf,
}

impl AgentBuilder {
    pub fn build_registry(&self) -> ToolRegistry { /* 统一注册 */ }
    pub fn build_components(&self) -> AgentComponents { /* ... */ }
    pub fn build_tui_runtime(&self) -> TuiRuntime { /* ... */ }
}
```

---

### 1.2 🔴 配置重复加载

**位置**: `orchestrator.rs:83`, `agent.rs:52`, `agent.rs:170`

**现状**: `load_config(None)` 在多个地方独立调用，每次重新读磁盘解析 TOML。`create_context_with_long_term()` 内部也单独 load_config，但调用方 `create_agent_components` 已经加载过一次。

**建议**: 配置只在入口加载一次，作为参数向下传递：
```rust
pub fn create_agent_components(cfg: &AppConfig, workspace: &Path, ...) -> AgentComponents
pub fn create_context_with_long_term(cfg: &AppConfig, max_turns: usize, ...) -> ContextManager
```

---

### 1.3 🟠 `react_loop` 参数爆炸 — 12 个参数

**位置**: `src/react/loop_.rs:77-90`

**现状**:
```rust
pub async fn react_loop(
    planner, executor, recovery, context, user_input,
    stream_tx, event_tx, cancel_token, critic, task_scheduler,
    system_prompt_override, allowed_tools,
) -> Result<ReactResult, AgentError>
```

**影响**: 难以维护、难以测试、调用方代码冗长。

**建议**: 引入 `ReactConfig` / `ReactSession` 结构体：
```rust
pub struct ReactSession<'a> {
    pub planner: &'a Planner,
    pub executor: &'a ToolExecutor,
    pub recovery: &'a RecoveryEngine,
    pub critic: Option<&'a Critic>,
    pub task_scheduler: Option<&'a TaskScheduler>,
    pub cancel_token: CancellationToken,
    pub stream_tx: Option<&'a broadcast::Sender<String>>,
    pub event_tx: Option<&'a mpsc::UnboundedSender<ReactEvent>>,
    pub system_prompt_override: Option<&'a str>,
    pub allowed_tools: Option<&'a [String]>,
}

pub async fn react_loop(
    session: &ReactSession<'_>,
    context: &mut ContextManager,
    user_input: &str,
) -> Result<ReactResult, AgentError>
```

---

### 1.4 🟠 CancellationToken 取消后不可恢复

**位置**: `src/core/session_supervisor.rs`

**现状**: 用户按 Ctrl+C 后 `cancel_token.cancel()` 将 token 永久取消。之后的新请求仍会在 `react_loop` 开头检查 `cancel_token.is_cancelled()` 并立即返回错误。

**建议**: 每次 `Submit` 创建新的 CancellationToken：
```rust
impl SessionSupervisor {
    pub fn new_cancel_token(&mut self) -> CancellationToken {
        self.cancel_token = CancellationToken::new();
        self.cancel_token.clone()
    }
}
```

---

## 二、异步与并发问题

### 2.1 🔴 `std::sync::Mutex` 包裹 SQLite — 阻塞 tokio runtime

**位置**: `src/core/orchestrator.rs:173`

**现状**:
```rust
let sqlite_persistence = Arc::new(Mutex::new(  // std::sync::Mutex!
    SqlitePersistence::new(&sqlite_db_path).ok()
));
```
在 `tokio::spawn` 的 async 块中调用 `sqlite_persistence_clone.lock()`，若 SQLite 操作耗时（如大量消息），会阻塞整个 tokio worker thread。

**建议**:
- 方案 A: 改用 `tokio::sync::Mutex`
- 方案 B: 将 SQLite 操作移至 `tokio::task::spawn_blocking`
- 方案 C (推荐): 迁移到 `sqlx` 的 async SQLite

---

### 2.2 🟠 同步文件 I/O 在 async 上下文中

**位置**: 多处

**受影响函数** (均在 react_loop 的 async 调用链中):
- `lessons_section()` → `load_lessons()` → `std::fs::read_to_string()`
- `procedural_section()` → `load_procedural()` → `std::fs::read_to_string()`
- `preferences_section()` → `load_preferences()` → `std::fs::read_to_string()`
- `append_lesson()`, `append_procedural()`, `append_preference()` → `std::fs::write()`
- `InMemoryVectorLongTerm::save_snapshot()` → `std::fs::write()`

**影响**: 每次 ReAct 循环都做同步文件读写，在高并发下会阻塞 tokio runtime。

**建议**: 
- 短期：用 `tokio::task::spawn_blocking` 包裹
- 长期：将文件记忆缓存到内存，定期异步 flush

---

### 2.3 🟡 TaskScheduler 的 `_active_tasks` 未使用

**位置**: `src/core/task_scheduler.rs:40`

**现状**: `_active_tasks: HashMap<TaskId, TaskKind>` 从未被写入或读取，只用了 semaphore。

**建议**: 要么实现完整的任务追踪（查看活跃任务、取消特定任务），要么删除该字段减少误导。

---

## 三、LLM 层问题

### 3.1 🔴 `complete_stream` 是假流式

**位置**: `src/llm/openai.rs:148-154`

**现状**:
```rust
async fn complete_stream(&self, messages: &[Message])
    -> Result<Pin<Box<dyn Stream<...> + Send>>, String> {
    let content = self.complete(messages).await?;  // 等全部完成
    Ok(Box::pin(stream::iter(vec![Ok(content)])))  // 包装成单元素流
}
```

**影响**: 用户看不到逐 token 输出，长回复时体验为「卡住→突然全部出现」；TUI 的 stream_rx 实际收到的是完整内容一次性推送。

**建议**: 使用 `async_openai` 的 `create_stream` API：
```rust
async fn complete_stream(&self, messages: &[Message]) -> Result<TokenStream, String> {
    let request = CreateChatCompletionRequestArgs::default()
        .model(&self.model)
        .messages(self.to_openai_messages(messages))
        .stream(true)  // 启用 SSE 流式
        .build()?;
    let stream = self.client.chat().create_stream(request).await?;
    // 将 ChatCompletionResponseStream 转为 String 流
    Ok(Box::pin(stream.filter_map(|result| async move {
        result.ok().and_then(|r| r.choices.first()?.delta.content.clone()).map(Ok)
    })))
}
```

---

### 3.2 🟠 LLM 错误类型为 `String`

**位置**: `src/llm/traits.rs:16-22`

**现状**:
```rust
async fn complete(&self, messages: &[Message]) -> Result<String, String>;
async fn complete_stream(&self, ...) -> Result<..., String>;
```

**影响**: 调用方无法区分超时 vs 认证失败 vs 模型不存在 vs 限流等，只能做字符串匹配。RecoveryEngine 收到的都是 `AgentError::LlmError(String)`，无法做精确恢复。

**建议**:
```rust
#[derive(Error, Debug)]
pub enum LlmError {
    #[error("Authentication failed")]
    AuthError,
    #[error("Rate limited, retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },
    #[error("Model not found: {model}")]
    ModelNotFound { model: String },
    #[error("Context length exceeded: {tokens} tokens")]
    ContextLengthExceeded { tokens: usize },
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Timeout after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
    #[error("API error: {0}")]
    ApiError(String),
}
```

---

### 3.3 🟠 无 LLM 调用重试与限流

**位置**: `src/llm/openai.rs`

**现状**: `complete()` 直接调用 API，失败即返回错误。无指数退避、无重试、无速率限制。

**建议**: 在 `LlmClient` 层或 wrapper 层加入：
- 指数退避重试 (429/5xx)
- 并发限制 (semaphore)
- 请求超时配置 (目前 `LlmTimeoutsSection` 已定义但未使用)

---

### 3.4 🟡 `LlmTimeoutsSection` 配置已定义但未使用

**位置**: `src/config.rs:358-372` (定义) vs `src/llm/openai.rs` (未引用)

**现状**: 配置文件中可设置 `request` 和 `stream` 超时，但 OpenAiClient 构造时未读取这些值，async_openai 使用默认超时。

---

## 四、ReAct 循环问题

### 4.1 🟠 JSON 解析脆弱

**位置**: `src/react/planner.rs:30-56`

**现状**: `parse_llm_output` 通过文本搜索 `{` 提取 JSON，`extract_first_json_object` 通过括号计数匹配。

**已知缺陷**:
1. 不处理 JSON 字符串内的 `{}`（如 `{"tool":"echo","args":{"text":"test {value}"}}`）
2. 不处理转义字符 `\{`
3. 含有多个 JSON 块时只取第一个，且没有验证机制
4. LLM 输出 `Response with some {random braces}` 会误判为 ToolCall 并解析失败

**建议**:
- 使用更健壮的 JSON 提取：考虑 `serde_json::StreamDeserializer` 或正则预过滤
- 添加 schema validation：解析后检查 `tool` 字段是否在已注册工具列表中
- 考虑用 LLM 的 function calling / tool_use API 替代自定义 JSON 解析

---

### 4.2 🟠 工具调用与对话历史混杂

**位置**: `src/react/loop_.rs:358-365`

**现状**: 工具调用结果作为 `Message::assistant` 和 `Message::user` 直接写入对话历史：
```rust
context.push_message(Message::assistant(format!("Tool call: {} | Result: {}", tc.tool, observation)));
context.push_message(Message::user(format!("Observation from {}: {}", tc.tool, observation)));
```

**影响**:
- 工具结果与真实用户对话混在一起，影响上下文质量
- LLM 可能把 observation 当成用户说的话
- 对话历史保存到 SQLite 时，工具记录污染用户对话
- 无法区分「用户消息」和「系统注入的工具结果」

**建议**: 扩展 `Role` 枚举或 `Message` 结构：
```rust
pub enum Role {
    User,
    Assistant,
    System,
    Tool { tool_name: String },  // 新增
}
// 或
pub struct Message {
    pub role: Role,
    pub content: String,
    pub metadata: Option<MessageMetadata>,  // tool_call_id, tool_name 等
}
```

---

### 4.3 🟡 Critic 使用同一 LLM — 成本翻倍且可能自我认同

**位置**: `src/react/critic.rs` & `src/core/orchestrator.rs:155`

**现状**: Critic 共享 Planner 的同一个 LLM 实例。每次工具调用后都额外做一次 LLM 调用来评估结果。

**影响**:
- Token 开销翻倍（每次工具调用多一轮 LLM）
- 同一个模型评估自己的输出，容易自我认同
- 无法配置是否启用 Critic、Critic 用哪个模型

**建议**:
- 配置化：`[critic] enabled = true, model = "deepseek-chat"`
- Critic 可用更轻量级的模型
- 按工具类型决定是否需要 Critic（如 echo 不需要，shell 需要）

---

## 五、记忆系统问题

### 5.1 🟠 长期记忆的「简单词重叠」检索质量低

**位置**: `src/memory/long_term.rs:40-108`

**现状**: `InMemoryLongTerm` 用空格分词 + 词集合交集数作为相似度。中文文本由于不按空格分词，检索基本失效。

**影响**: 项目文档和用户交互以中文为主，长期记忆检索形同虚设。

**建议**:
- 短期：加入中文分词（jieba-rs）
- 中期：默认启用向量检索，支持本地嵌入模型（如 fastembed-rs）避免依赖外部 API
- 长期：引入 RAG pipeline

---

### 5.2 🟠 每次 ReAct 循环都全量拼接记忆到 system prompt

**位置**: `src/react/loop_.rs:134-158`

**现状**: 每步都调用 `working_memory_section()` + `long_term_section()` + `lessons_section()` + `procedural_section()` + `preferences_section()`，其中后三者每次都读文件。

**影响**:
- 不必要的文件 I/O（每步都读）
- 随着记忆增长，system prompt 无限膨胀，浪费 token
- 没有 token 预算控制

**建议**:
- 缓存文件内容，仅在变更时重新读取
- 为 system prompt 设置 token 预算，各记忆段按优先级竞争
- lessons/procedural/preferences 在会话开始时加载一次，循环内只更新 working memory

---

### 5.3 🟡 ConversationMemory 剪枝策略过于简单

**位置**: `src/memory/conversation.rs:94-99`

**现状**: 超过 `max_turns * 2` 条消息时直接 `drain` 最旧的。

**影响**: 丢弃的可能是关键上下文（如 system 消息、第一条用户指令）。

**建议**:
- 保留 system 消息不被剪枝
- 按重要性评分决定保留哪些（工具结果可优先丢弃）
- 剪枝前将丢弃内容摘要写入长期记忆

---

## 六、工具系统问题

### 6.1 🟠 `tool_call_schema_json()` 是全局静态 — 与实际注册的工具不匹配

**位置**: `src/tools/schema.rs`

**现状**: Schema 是编译期硬编码的 JSON 字符串，不反映运行时实际注册了哪些工具。

**影响**: LLM 可能调用 schema 中有但 registry 中没注册的工具（如 TUI 没注册 code_edit 但 schema 里有），导致 HallucinatedTool 错误。

**建议**: 从 `ToolRegistry` 动态生成 schema：
```rust
impl ToolRegistry {
    pub fn to_schema_json(&self) -> String {
        let tools: Vec<_> = self.tools.values()
            .map(|t| json!({ "name": t.name(), "description": t.description() }))
            .collect();
        serde_json::to_string_pretty(&tools).unwrap()
    }
}
```

---

### 6.2 🟡 Tool trait 缺少参数 schema

**位置**: `src/tools/registry.rs:13-18`

**现状**:
```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn execute(&self, args: Value) -> Result<String, String>;
}
```

**影响**: LLM 只看到工具名和描述，不知道参数格式。正确的参数全靠 LLM 猜测和 system prompt 中的硬编码 schema。

**建议**: 添加 `parameters_schema` 方法：
```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;  // JSON Schema
    async fn execute(&self, args: Value) -> Result<String, String>;
}
```

---

### 6.3 🟡 工具错误也是 `String`

**位置**: `src/tools/registry.rs:17`

**现状**: `execute` 返回 `Result<String, String>`，工具执行错误为纯字符串。

**建议**: 引入 `ToolError` 枚举，区分参数错误、超时、权限拒绝、内部错误等。

---

## 七、可观测性与运维

### 7.1 🟠 可观测性为占位符

**位置**: `src/observability/mod.rs`

**现状**:
```rust
pub fn init_metrics() {
    tracing::info!("Metrics initialized (placeholder)");
}
```

**缺失项**:
- 无结构化 metrics（LLM 调用次数/延迟/token 消耗/错误率）
- 无 tracing spans（无法跟踪单次请求的完整生命周期）
- 无性能采样
- 无告警阈值

**建议**:
- 使用 `tracing::instrument` 为关键函数添加 spans
- 引入 `metrics` crate + prometheus exporter
- 关键指标：LLM latency p50/p99, tool execution time, token usage per session, error rate

---

### 7.2 🟡 无优雅关闭

**现状**: 各 binary 无 graceful shutdown：
- 向量快照不在退出时保存
- SQLite 连接无显式关闭
- broadcast channel 可能丢失最后几条消息

**建议**: 添加 `tokio::signal::ctrl_c()` 处理，触发 cleanup：保存向量快照、flush 日志、关闭连接。

---

## 八、测试覆盖

### 8.1 🔴 测试覆盖严重不足

**现状**: 整个项目仅 **8 个单元测试**：
- `config::tests::test_default_app_config` (1)
- `memory::long_term::tests::test_cosine_similarity` (1)
- `tools::code_edit::tests::*` (3)
- `tools::code_read::tests::*` (1)
- `memory::markdown_store::tests::*` (2, 推测)
- 0 个集成测试
- 0 个测试覆盖 react_loop, planner, critic, orchestrator, recovery

**缺失的关键测试**:

| 模块 | 应有测试 |
|------|----------|
| `react_loop` | 正常完成、工具调用、取消、超过最大步数、恢复重试 |
| `parse_llm_output` | 纯文本→Response、JSON→ToolCall、混合文本、嵌套 JSON、格式错误 |
| `RecoveryEngine` | 每种 AgentError → 对应 RecoveryAction 的映射 |
| `ContextManager` | 记忆拼接、lessons 注入、compaction、长期记忆交互 |
| `ConversationMemory` | 剪枝边界、push/clear/set_messages |
| `ToolExecutor` | 超时、未知工具、成功执行 |
| `SqlitePersistence` | CRUD、并发读写 |
| `LlmClient` (Mock) | 确保 Mock 行为用于测试 |
| 集成测试 | 完整 submit→react→response 流程 |

**建议**: 优先补充 `parse_llm_output` 和 `RecoveryEngine` 的单元测试（纯函数，不依赖外部），再逐步覆盖核心流程。

---

## 九、代码质量

### 9.1 🟡 Clippy 32 个 warning

**现状**: `cargo clippy` 报告 32 个 warning，包括：
- `unnecessary_lazy_evaluations` (git_diff.rs)
- `too_many_arguments` (render.rs)
- 23 个可自动修复的 suggestion

**建议**: 运行 `cargo clippy --fix` 处理可自动修复的，手动处理 `too_many_arguments` (与 4.3 react_loop 参数结构体化一致)。

---

### 9.2 🟡 不同二进制之间缺少共享抽象

**位置**: `src/bin/web.rs`, `src/bin/whatsapp.rs`, `src/bin/lark.rs`, `src/bin/gateway.rs`

**现状**: 每个 binary 各自初始化 Agent（重复的 workspace 设置、config 加载、prompt 读取）。

**建议**: 提取 `fn init_agent_runtime(config_override: Option<PathBuf>) -> AgentRuntime` 统一入口。

---

### 9.3 🟡 `Planner::summarize` 与 `compact_context` 紧耦合

**位置**: `src/react/planner.rs:127-138`, `src/react/loop_.rs:54-72`

**现状**: `summarize` 是 Planner 的方法，但语义上属于记忆管理。`compact_context` 是独立函数但直接操作 ContextManager 内部。

**建议**: 将 compaction 逻辑封装为 ContextManager 的方法：
```rust
impl ContextManager {
    pub async fn compact(&mut self, llm: &dyn LlmClient) -> Result<(), AgentError> { ... }
}
```

---

## 十、改进优先级路线图

### Phase 1 — 紧急修复 (1-2 周)

| # | 问题 | 章节 | 预估工时 |
|---|------|------|---------|
| 1 | 统一 Agent 初始化，消除 TUI 与 Headless 的工具差异 | 1.1 | 4h |
| 2 | `std::sync::Mutex` → `tokio::sync::Mutex` | 2.1 | 1h |
| 3 | CancellationToken 每次 Submit 重建 | 1.4 | 1h |
| 4 | 配置单次加载向下传递 | 1.2 | 2h |
| 5 | 消除 Clippy warnings | 9.1 | 1h |

### Phase 2 — 核心改进 (2-4 周)

| # | 问题 | 章节 | 预估工时 |
|---|------|------|---------|
| 6 | 实现真正的流式 LLM 输出 | 3.1 | 8h |
| 7 | LLM 错误类型化 + 重试策略 | 3.2, 3.3 | 6h |
| 8 | react_loop 参数结构体化 | 1.3 | 3h |
| 9 | Tool schema 从 Registry 动态生成 | 6.1 | 4h |
| 10 | 核心模块测试覆盖 (parse_llm_output, RecoveryEngine, ContextManager) | 8.1 | 8h |
| 11 | 工具调用与对话历史分离 (Role::Tool) | 4.2 | 4h |

### Phase 3 — 质量提升 (1-2 月)

| # | 问题 | 章节 | 预估工时 |
|---|------|------|---------|
| 12 | 异步文件 I/O 改造 | 2.2 | 6h |
| 13 | 长期记忆中文分词 + 本地嵌入支持 | 5.1 | 12h |
| 14 | 记忆 token 预算控制 | 5.2 | 6h |
| 15 | 可观测性：tracing spans + metrics | 7.1 | 8h |
| 16 | Tool trait 添加 parameters_schema | 6.2 | 6h |
| 17 | Critic 配置化与模型分离 | 4.3 | 4h |
| 18 | 优雅关闭 | 7.2 | 3h |
| 19 | JSON 解析健壮性 / 考虑 function calling | 4.1 | 8h |
| 20 | 集成测试：完整 submit→react→response | 8.1 | 8h |

### Phase 4 — 长期演进

- ConversationMemory 智能剪枝 (5.3)
- 迁移到 sqlx async SQLite
- 引入 RAG pipeline
- 多模型 router (按任务类型选模型)
- Plugin 系统标准化 (WASM / gRPC)

---

## 附录：文件索引

| 文件 | 相关问题 |
|------|---------|
| `src/core/orchestrator.rs` | 1.1, 1.2, 2.1, 1.4 |
| `src/agent.rs` | 1.1, 1.2 |
| `src/react/loop_.rs` | 1.3, 4.2, 5.2 |
| `src/react/planner.rs` | 4.1, 9.3 |
| `src/react/critic.rs` | 4.3 |
| `src/llm/traits.rs` | 3.2 |
| `src/llm/openai.rs` | 3.1, 3.3, 3.4 |
| `src/memory/long_term.rs` | 5.1 |
| `src/memory/conversation.rs` | 5.3 |
| `src/memory/persistence.rs` | 2.1 |
| `src/tools/registry.rs` | 6.1, 6.2, 6.3 |
| `src/tools/schema.rs` | 6.1 |
| `src/core/session_supervisor.rs` | 1.4 |
| `src/core/task_scheduler.rs` | 2.3 |
| `src/observability/mod.rs` | 7.1 |
| `src/config.rs` | 3.4 |
