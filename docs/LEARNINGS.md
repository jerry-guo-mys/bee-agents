# 自我改进智能体（.learnings）

Bee 可将学习内容、错误和修正记录到 `workspace/.learnings/` 下的 Markdown 文件中，实现持续改进。

## 目录与文件

| 文件 | 触发场景 |
|------|----------|
| **ERRORS.md** | 命令/操作失败；API 或外部工具失败（含集成详情） |
| **LEARNINGS.md** | 用户纠正（correction）、知识过时（knowledge_gap）、发现更好方法（best_practice） |
| **FEATURE_REQUESTS.md** | 用户提出缺失的功能需求 |

## 自动记录（当前实现）

- **工具执行失败**：ReAct 中任意工具调用失败时，会追加到 `ERRORS.md`（工具名 + 失败原因）。
- **Critic 纠正**：当 Critic 判定工具结果不符合目标并给出修正建议时，会追加到 `LEARNINGS.md`，分类为 `correction`。

## 分类与约定

- **LEARNINGS.md** 条目带分类标签：`[correction]`、`[knowledge_gap]`、`[best_practice]`。
- 与已有条目相似时，可使用 **See Also** 链接，并考虑提升优先级。
- 所有条目带时间戳（`YYYY-MM-DD HH:MM`），便于追溯。

## 提升到工作区

当学习内容被证明**广泛适用**时，可提升到 `workspace/` 根目录的以下文件（供 system prompt / 规划 长期引用）：

| 文件 | 用途 | 示例 |
|------|------|------|
| **SOUL.md** | 行为模式 | 简洁明了，避免免责声明 |
| **AGENTS.md** | 工作流改进 | 长任务生成子代理 |
| **TOOLS.md** | 工具技巧 | Git push 需要先配置认证 |

- `bee::memory::promote_to_soul(workspace, content)` — 追加到 SOUL.md
- `bee::memory::promote_to_agents(workspace, content)` — 追加到 AGENTS.md
- `bee::memory::promote_to_tools(workspace, content)` — 追加到 TOOLS.md

提升可由人工或后续策略触发（例如：LEARNINGS.md 某条被多次引用、或标注为高优先级时自动提升）。

## 扩展

- **功能需求**：可通过 `bee::memory::record_feature_request(workspace, description)` 在业务逻辑中写入 `FEATURE_REQUESTS.md`。
- **知识过时 / 更好方法**：可通过 `bee::memory::record_learning(workspace, "knowledge_gap" | "best_practice", content, see_also)` 写入 `LEARNINGS.md`。
- 后续可增加从 system prompt 或工具中读取 `.learnings` 与 `SOUL.md` / `AGENTS.md` / `TOOLS.md`，供规划与反思使用。
