# Bee 记忆存储（Markdown 文件）

Bee 使用**简单透明的纯文本（Markdown）文件**存储短期与长期记忆，便于人工查看与版本管理。

## 目录结构

在 `workspace/memory/` 下：

```
memory/
├── logs/                  # 短期记忆：按日期组织的对话日志
│   ├── 2025-02-01.md
│   └── ...
├── long-term.md           # 长期记忆（BM25 模式）：持久知识、用户偏好
├── vector_snapshot.json   # 向量长期记忆快照（启用 [memory].vector_enabled 时）
└── heartbeat_log.md       # 心跳结果沉淀（bee-web 启用心跳时）
```

## 短期记忆（日志）

- **路径**：`memory/logs/YYYY-MM-DD.md`
- **用途**：按日期记录当日对话，便于回溯与审计。
- **格式**：每次会话追加一段 Markdown，包含 Session ID、日期、User/Assistant 消息。

示例：

```markdown
## Session abc-123 (2025-02-01)

### User
今天天气怎么样？

### Assistant
我无法直接获取实时天气……

---

## Session def-456 (2025-02-01)
…
```

- **写入时机**：Web 端在保存会话到磁盘时（`save_session_to_disk`）同时追加到当日日志。

## 长期记忆（持久文件）

- **路径**：`memory/long-term.md`（或启用向量时为内存向量 + `memory/vector_snapshot.json` 快照）
- **用途**：存储跨会话的持久知识、用户偏好等，供 ReAct 检索后注入 Prompt。
- **格式**：按块追加，每块带时间戳标题。

示例：

```markdown
## 2025-02-01 12:00

用户偏好使用 Python，常用 fastapi。

## 2025-02-01 12:05

项目 Bee 的 workspace 位于 ~/projects/bee/workspace。
```

- **写入时机**：ReAct 循环在最终回复后调用 `push_to_long_term` 时。
- **检索**：当前实现为 **BM25 风格关键词检索**（按块切分、词重叠 + 文档长度归一化），启动时从文件加载到内存索引。

## 向量长期记忆（可选）

当 `config [memory].vector_enabled = true` 时，长期记忆使用**嵌入 API + 内存向量**（`InMemoryVectorLongTerm`）：

- **写入**：每次 `push_to_long_term` 时调用 OpenAI 兼容的 `/embeddings` 将文本转为向量并存入内存。
- **检索**：对用户输入做嵌入，按**余弦相似度**返回 top-k 文本片段，拼入 Prompt。
- **持久化**：向量会定期（每 5 分钟）保存到 `memory/vector_snapshot.json`，启动时自动加载，避免重启丢失。bee-web 使用共享向量实例并负责快照保存。
- **配置**：`[memory].embedding_model`（默认 `text-embedding-3-small`）；可选 `embedding_base_url`、`embedding_api_key` 以与 LLM 解耦。

未启用或未配置 API Key 时回退为 `FileLongTerm`（BM25）。

## 检索与扩展（向量 + BM25）

1. **当前**：BM25（`FileLongTerm`）或向量（`InMemoryVectorLongTerm`，见上），二选一。
2. **可选扩展**：接入 qdrant 等外部向量库（`[memory].qdrant_url` 已预留）；混合检索（向量 + BM25）等。

## 配置与位置

- 记忆根目录由 **workspace** 推导：`workspace/memory/`。
- Web 端使用 `workspace` 为当前目录下的 `workspace`；TUI/WhatsApp 若未传 workspace，长期记忆仍为内存实现（`InMemoryLongTerm`），不写文件。

## 忽略与备份

- `workspace/` 已在 `.gitignore` 中，故 `workspace/memory/` 不会被提交。
- 如需备份，可直接复制 `memory/` 目录或对 `long-term.md` 与 `logs/*.md` 做版本管理。
