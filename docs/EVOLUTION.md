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
