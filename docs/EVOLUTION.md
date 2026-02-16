# Agent 自我进化设计

## 1. 什么是「自我进化」

在本项目中，**自我进化**指：Agent 在不改代码、不重训模型的前提下，通过**记忆、反馈与规则积累**，让后续行为更符合用户习惯、减少重复错误、并更好利用历史经验。

## 2. 已有基础（可直接利用）

| 能力 | 位置 | 用途 |
|------|------|------|
| **长期记忆** | `LongTermMemory` / `FileLongTerm` | 存知识、摘要，按 query 检索，注入 Prompt |
| **短期日志** | `memory/logs/YYYY-MM-DD.md` | 按日记录对话，供整理 |
| **记忆整理** | `consolidate_memory()` | 将近期日志归纳写入 `long-term.md` |
| **Working Memory** | `WorkingMemory` (goal / attempts / failures) | 单轮内已尝试与失败，避免重复犯错 |
| **Recovery** | `RecoveryEngine` | 根据错误类型决定重试 / 问用户 / 中止 |
| **Critic** | 可选 | 对工具调用结果做合理性检查 |

进化 = 把「单轮」的有效信息沉淀到「跨轮 / 跨会话」可复用的结构里。

## 3. 进化维度与实现思路

### 3.1 从失败中沉淀规则（Failure → Rules）

- **现象**：工具幻觉、超时、错误参数等重复出现。
- **做法**：
  - 在 `RecoveryEngine::handle` 或 ReAct 循环中，当某类错误发生（如 `HallucinatedTool`）时，不仅返回 `AskUser`，还可**写一条「教训」**到持久化存储（例如 `memory/lessons.md` 或 long-term 的专用块）。
  - 内容示例：`"Only use tools: cat, ls, shell, search, echo. Do not invent tool names."`
  - 在拼 system prompt 时，**固定追加「Lessons / 行为约束」**：读取 `lessons` 或 long-term 中「教训」类条目，拼成 `## 行为约束\n...`，让模型在后续对话中遵守。

### 3.2 用户偏好与显式记忆（Preference & Explicit Memory）

- **现象**：用户希望 Agent 记住「我喜欢简短回答」「不要用红色」等。
- **做法**：
  - **显式指令**：识别用户输入中的「记住：xxx」「以后都 xxx」，将 xxx 写入长期记忆（或专用 `memory/preferences.md`），并在后续 `long_term_section(query)` 中优先检索「偏好」类内容。
  - **结构化偏好**：在配置或 `memory/preferences.md` 中维护键值型偏好（如 `answer_style: concise`），拼 system 时注入一句「User prefers: ...」。

### 3.3 整理与摘要的智能化（Smarter Consolidation）

- **现象**：当前 `consolidate_memory` 主要是截断与去内部消息，摘要能力弱。
- **做法**：
  - 对每个 `memory/logs/YYYY-MM-DD.md` 调用 LLM 做**摘要**（或关键事实抽取），再写入 long-term；摘要可带「主题 / 标签」，便于检索。
  - 可选：只对「高价值」会话（如用户点赞、长对话）做摘要，避免噪音。

### 3.4 Critic 结论沉淀（Critic → Lessons）

- **现象**：Critic 判断某次工具调用不合理并给出修正建议，但仅影响当轮。
- **做法**：
  - 若 Critic 多次对同类问题给出相似建议，可将建议归纳为一条规则，写入 `lessons` 或 long-term（例如「搜索前先确认域名在白名单内」），供后续 system 使用。

### 3.5 工具使用统计与策略偏好（Tool Stats）

- **现象**：某些工具组合或调用顺序更易成功。
- **做法**：
  - 在 ToolExecutor 或 ReAct 中轻量记录：`(session_id, tool_name, success)`（可选带 goal 摘要）；定期或按会话汇总为「推荐策略」文本，写入 long-term 或 lessons，检索时注入 Prompt（例如「类似任务下，先 search 再 cat 通常更有效」）。

## 4. 推荐实现顺序

1. **Lessons 文件 + System 注入**  
   - 新增 `memory/lessons.md`（或 long-term 中「教训」块），在 `Planner` 拼 system 时追加 `## 行为约束 / Lessons\n` + 文件内容。  
   - 可选：在 Recovery 处理 `HallucinatedTool` 等时，自动追加一条对应教训（或由用户确认后写入）。

2. **显式用户偏好**  
   - 解析「记住：xxx」或调用 `/api/preference`，写入 `memory/preferences.md` 或 long-term；检索 long-term 时优先包含「偏好」片段。

3. **整理时用 LLM 摘要**  
   - 在 `consolidate_memory` 中，对每日日志调用 LLM 生成简短摘要再写入 long-term，便于后续检索到「高信息量」内容。

4. **Critic 建议写入 Lessons**  
   - Critic 输出若为「修正建议」，在用户确认或自动满足一定条件时，写入 lessons。

## 5. 与现有架构的衔接

- **不改 ReAct 主流程**：进化全部通过「写记忆 / 写 lessons」+「拼 system 时多读一块」完成。
- **长期记忆**：继续用现有 `FileLongTerm`（或后续向量库）；lessons / preferences 可视为**带类型的长期块**（类型在检索时过滤或优先）。
- **配置**：可在 `config/default.toml` 中增加 `[memory]` 或 `[evolution]`，例如：
  - `lessons_path = "memory/lessons.md"`
  - `auto_lesson_on_hallucination = true`

按上述顺序实现，即可在现有 Bee 架构上实现「越用越听话、越少重复错」的自我进化效果。

---

## 6. 已实现：行为约束 / Lessons（第一步）

- **文件**：`workspace/memory/lessons.md`
- **作用**：该文件内容会在每次规划时拼入 system prompt 的「## 行为约束 / Lessons」段落，模型会遵守其中规则。
- **用法**：新建或编辑该文件，每行一条规则或一段说明，例如：
  ```markdown
  仅使用工具：cat、ls、shell、search、echo；不要编造不存在的工具名。
  调用工具时必须输出合法 JSON：{"tool":"工具名","args":{...}}。
  用户说「记住：xxx」时，将 xxx 写入你的回复并可在后续对话中引用。
  ```
- **生效**：无需重启，保存后下一轮对话即会带上最新内容。未创建该文件时不会注入任何内容。

---

## 7. 已实现：程序记忆 (Procedural Memory)

- **文件**：`workspace/memory/procedural.md`
- **作用**：记录工具调用失败（工具名 + 错误原因），并在每次规划时拼入 system prompt 的「## 程序记忆 / 工具使用经验」段落，减少重复错误。
- **写入**：ReAct 循环中工具执行失败时自动追加一条记录（`append_procedural_record`）。
- **生效**：无需重启；未创建该文件时首次失败会自动创建并写入。

---

## 8. 已实现：Context Compaction（上下文压缩）

- **机制**：当对话条数超过阈值（默认 24）时，在规划前自动执行一次压缩：用 LLM 对当前对话生成摘要，写入长期记忆（`push_to_long_term`），并将当前消息替换为一条「Previous conversation summary」的 system 消息（`set_messages`），避免 token 溢出。
- **手动触发**：Web API `POST /api/compact`，请求体 `{ "session_id": "..." }`。
- **代码**：`Planner::summarize()`、`compact_context()`、`ConversationMemory::set_messages()`；ReAct 循环内按 `COMPACT_THRESHOLD` 自动调用。

---

## 9. 已实现：显式用户偏好 (Preferences)

- **文件**：`workspace/memory/preferences.md`
- **作用**：用户说「记住：xxx」时，自动将 xxx 写入该文件并同步到长期记忆；每次规划时拼入 system prompt 的「## 用户偏好 / Preferences」段落，模型会遵守。
- **识别**：ReAct 循环在收到用户输入后检测「记住」+「：」或「:」后的内容（如「记住：我喜欢简短回答」），提取并调用 `append_preference` + `push_to_long_term`。
- **生效**：无需重启；未创建该文件时首次「记住：」会自动创建并写入。

---

## 10. 已实现：HallucinatedTool 自动写入 Lesson

- **机制**：当模型调用了不存在的工具（HallucinatedTool）时，在返回错误前自动向 `memory/lessons.md` 追加一条教训，内容为「仅使用以下已注册工具：cat、ls、…；不要编造不存在的工具名（例如曾误用「xxx」）。」
- **作用**：后续对话中该段落会随 lessons 注入 system，减少重复幻觉。
- **代码**：`ContextManager::append_hallucination_lesson(hallucinated_tool, valid_tools)`，在 ReAct 循环检测到 HallucinatedTool 时调用。

---

## 11. 已实现：Critic 建议写入 Lessons（§3.4）

- **机制**：当 Critic 对工具结果给出修正建议（`CriticResult::Correction(suggestion)`）时，除注入当轮 user 消息外，自动向 `memory/lessons.md` 追加一行「Critic 建议：{suggestion}」。
- **作用**：后续对话中该内容会随 lessons 注入 system，减少同类工具使用错误。
- **代码**：`ContextManager::append_critic_lesson(suggestion)`，在 ReAct 循环 Critic 返回 Correction 时调用。

---

## 12. 已实现：整理时用 LLM 摘要（§3.3）

- **机制**：对近期每日日志（`memory/logs/YYYY-MM-DD.md`）不再仅做截断，而是调用 LLM 对每日本内容生成简短摘要后写入长期记忆（块标题为「整理 YYYY-MM-DD（LLM 摘要）」）。
- **入口**：`agent::consolidate_memory_with_llm(planner, workspace, since_days)`；Web API `POST /api/memory/consolidate-llm?since_days=7`。
- **依赖**：`memory::list_daily_logs_for_llm` 列出待整理日志；`Planner::summarize` 对单日内容做摘要。

---

## 13. 已实现：工具使用统计与策略偏好（§3.5）

- **工具成功记录**：配置 `[evolution].record_tool_success = true` 时，每次工具调用成功也会写入 `memory/procedural.md`（与失败记录一致，便于后续检索「哪些工具常用且成功」）。默认 `false` 以减少文件噪音。
- **策略沉淀**：当一轮对话以「直接回复用户」成功结束时，将「本轮目标 + 使用的工具列表」写入长期记忆，格式为 `Session strategy: goal "..."; tools used: cat, search.`，供后续 `long_term_section(query)` 检索到类似任务下的工具组合。
- **代码**：`WorkingMemory::tool_names_used()` 从本轮的 `attempts`（`tool -> observation`）提取工具名；`ContextManager::push_session_strategy_to_long_term(goal, tool_names)`、`with_record_tool_success`；ReAct 成功返回前调用策略写入，工具成功时按配置调用 `append_procedural_record(..., true, "ok")`。

---

## 14. 已实现：心跳机制（后台自主循环）

- **机制**：bee-web 启动时若配置 `[heartbeat] enabled = true`，会 spawn 一个后台任务，按 `interval_secs`（默认 300 秒）周期执行一次「心跳」：用 `create_context_with_long_term` 构建上下文，向 Agent 发送固定提示（Heartbeat prompt），让其根据长期记忆与当前状态检查待办或需跟进事项；若有则输出简短建议，若无则回复 OK；可使用 cat/ls 查看 workspace 下 memory 或任务文件。
- **配置**：`config/default.toml` 中 `[heartbeat]` 段：`enabled`（是否启用）、`interval_secs`（间隔秒数）。默认关闭。
- **代码**：`src/bin/web.rs` 启动时 `load_config` 读取配置，若 `heartbeat.enabled` 则 `tokio::spawn` 定时循环，每次 tick 调用 `process_message(..., HEARTBEAT_PROMPT)`，结果以 `tracing::info` / `tracing::warn` 打日志。

---

## 15. 已实现：技能插件（Agent 动态注册新工具）

- **机制**：在 `config/default.toml` 中通过 `[[tools.plugins]]` 配置额外工具；每项指定 `name`、`description`、`program`、`args`（参数模板）。模板中 `{{workspace}}` 替换为沙箱根路径，`{{key}}` 从 LLM 传入的 `args` JSON 中取对应 key。执行时无 shell，直接 `exec` 程序 + 替换后的参数，带全局工具超时与审计日志。
- **注册**：`create_agent_components`（agent.rs）与 TUI 侧 `create_agent`（orchestrator.rs）在注册内置工具后，遍历 `cfg.tools.plugins` 并 `register(PluginTool::new(...))`，故 Web / TUI / WhatsApp 均支持插件。
- **代码**：`src/tools/plugin.rs`（`PluginTool`）、`src/config.rs`（`PluginEntry`、`ToolsSection.plugins`）。

---

## 16. 已实现：向量检索（长期记忆 + 嵌入 API）

- **机制**：当 `config [memory].vector_enabled = true` 时，长期记忆使用 `InMemoryVectorLongTerm`：写入时调用 OpenAI 兼容的 `/embeddings` 将文本转为向量并存入内存，检索时对 query 做嵌入并按余弦相似度返回 top-k 文本片段。与 LLM 共用 `base_url`、`OPENAI_API_KEY`；嵌入模型由 `[memory].embedding_model` 指定（默认 `text-embedding-3-small`）。
- **配置**：`config/default.toml` 中 `[memory]`：`vector_enabled`、`embedding_model`、`qdrant_url`（预留）。
- **代码**：`src/llm/embedding.rs`（`EmbeddingProvider`、`OpenAiEmbedder`、`create_embedder_from_config`）；`src/memory/long_term.rs`（`InMemoryVectorLongTerm`、余弦相似度）；`create_context_with_long_term` 在 `vector_enabled` 且可创建 embedder 时选用向量后端，否则回退 FileLongTerm。
 - **预留**：`qdrant_url` 与 qdrant-client 接入为可选扩展，当前为纯内存向量存储。

---

## 17. 已实现：自主迭代系统（安全可控的自我改进）

Bee 实现了完整的自主迭代系统，允许 Agent 分析自身代码、生成改进计划、执行修改并验证结果，同时提供全面的安全控制和人工干预机制。

### 核心组件

1. **分析器 (SelfAnalyzer)**: 扫描代码库，识别性能、可读性、文档、测试等方面的问题，生成质量评分
2. **规划器 (ImprovementPlanner)**: 根据分析结果制定具体的改进步骤，确保步骤原子性、可验证性
3. **执行引擎 (ExecutionEngine)**: 安全执行改进步骤，包含审批流程、安全验证、回滚机制
4. **调度引擎 (EvolutionEngine)**: 控制迭代频率，支持手动、间隔、每日、每周调度
5. **进化循环 (EvolutionLoop)**: 协调整个迭代过程，管理迭代周期和结果跟踪

### 配置说明

完整配置位于 `config/default.toml` 的 `[evolution]` 段：

```toml
[evolution]
# 基础设置
enabled = true                    # 是否启用自主迭代
max_iterations = 10               # 单次运行最大迭代次数
target_score_threshold = 0.8      # 目标质量分数阈值
auto_commit = true                # 是否自动提交 Git
require_approval = false          # 是否需要人工确认（向后兼容）
focus_areas = ["performance", "readability", "documentation", "testing"]  # 重点改进领域

# 调度控制
schedule_type = "manual"          # manual, interval, daily, weekly
schedule_interval_seconds = 86400 # 间隔调度秒数（默认24小时）
schedule_time = "02:00"           # 每日/每周调度的具体时间（HH:MM）
max_iterations_per_period = 3     # 每个周期最大迭代次数
cooldown_seconds = 300            # 失败后的冷却时间（秒）

# 审批控制
approval_mode = "none"            # none, console, prompt, webhook
approval_timeout_seconds = 3600   # 等待审批的超时时间（秒）
# approval_webhook_url = "..."    # Webhook URL（用于外部审批系统）
require_approval_for = ["critical"] # 需要审批的操作类型

# 安全限制
safe_mode = "strict"              # strict, balanced, permissive
allowed_directories = ["./src"]   # 允许修改的目录白名单
restricted_files = ["Cargo.toml", "src/main.rs"] # 禁止修改的关键文件
max_file_size_kb = 1024           # 单次修改最大文件大小（KB）
allowed_operation_types = ["add", "replace"] # 允许的操作类型
rollback_enabled = true           # 失败时自动回滚
backup_before_edit = true         # 编辑前创建备份
```

### 安全特性

1. **目录白名单**: 只能修改 `allowed_directories` 中的文件
2. **文件黑名单**: 禁止修改 `restricted_files` 中的关键文件
3. **操作类型限制**: 仅允许 `allowed_operation_types` 中的操作（add/replace/remove/rename）
4. **文件大小限制**: 防止修改过大文件（默认1MB）
5. **审批工作流**: 支持控制台、带超时的提示、Webhook 三种审批模式
6. **自动回滚**: 操作失败时自动恢复原始文件
7. **备份机制**: 编辑前自动创建备份副本

### 调度类型

- **manual (手动)**: 仅通过 API 或命令行手动触发
- **interval (间隔)**: 每 N 秒运行一次（默认24小时）
- **daily (每日)**: 每天指定时间运行（默认02:00）
- **weekly (每周)**: 每周指定时间运行（目前按7天间隔）

### 审批模式

- **none (无)**: 无需审批，自动执行
- **console (控制台)**: 在控制台交互式询问用户是否批准
- **prompt (提示超时)**: 在控制台询问，超时后自动拒绝（默认1小时）
- **webhook (Webhook)**: 向指定 URL 发送审批请求，等待批准响应

### 使用方式

1. **命令行测试**:
   ```bash
   cargo run --bin bee-evolution
   ```

2. **集成到主程序**:
   Bee 主程序已集成进化循环，可通过配置启用。

3. **手动触发**:
   ```rust
   let mut evolution_loop = EvolutionLoop::new(llm, executor, config, project_root);
   let results = evolution_loop.run().await?;
   ```

4. **目标迭代**:
   ```rust
   let result = evolution_loop.run_targeted_iteration(
       vec!["src/lib.rs".to_string()],
       "优化错误处理"
   ).await?;
   ```

### 验证与质量保证

1. **步骤验证**: 每个步骤执行后立即验证代码可编译
2. **测试运行**: 迭代完成后运行完整测试套件
3. **质量评估**: 计算改进后的代码质量评分
4. **经验学习**: 记录成功和失败的教训，用于后续迭代优化

### 监控与调试

- 每个步骤都有详细日志输出
- 迭代结果包含成功/失败状态、质量分数、测试结果
- 失败时记录具体原因和教训
- 支持通过配置调整日志详细程度

### 设计原则

1. **安全第一**: 所有修改都经过多层安全验证
2. **渐进改进**: 小步快跑，每次迭代只做有限修改
3. **人工可控**: 提供完整的审批和干预机制
4. **可观测性**: 完整的日志和结果跟踪
5. **可恢复性**: 失败时自动回滚，不影响系统稳定性

该系统使 Bee 能够持续自我改进，同时确保安全性和可控性，满足企业级部署的要求。
