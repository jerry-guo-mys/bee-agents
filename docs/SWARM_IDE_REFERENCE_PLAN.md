# Bee × swarm-ide 参考实施计划

> 基于 [swarm-ide](https://github.com/chmod777john/swarm-ide) 源码分析，为 Bee 提供「1 对 1 + 群聊 + 动态 Agent」的参考实现路线图。

---

## 一、swarm-ide 核心架构解析

### 1.1 极简原语（作为 Tool 暴露给 LLM）

| 原语 | 参数 | 行为 |
|------|------|------|
| `create` | `role`, `guidance` | 创建 sub-agent，返回 `agentId`，自动建立与 creator 的 P2P 群 |
| `send` | `to`, `content` | 向 agent_id 发私信，自动创建/复用 P2P 群 |
| `list_agents` | - | 列出 workspace 内所有 agent |
| `create_group` | `memberIds`, `name` | 创建群聊（≥2 人） |
| `send_group_message` | `groupId`, `content` | 群发消息 |
| `send_direct_message` | `toAgentId`, `content` | 向某 agent 发私信（与 send 类似） |
| `list_groups` | - | 列出当前 agent 可见的群 |
| `list_group_members` | `groupId` | 列出群成员 |
| `get_group_messages` | `groupId` | 拉取群消息历史 |

### 1.2 数据模型（PostgreSQL + Drizzle）

```
workspaces
  └── agents (id, workspaceId, role, parentId, guidance, ...)
  └── groups (id, workspaceId, name, ...)
        └── group_members (groupId, userId, lastReadMessageId, ...)
        └── messages (id, groupId, senderId, content, sendTime, ...)
```

- **Agent**：每个 agent 有 `role`、`parentId`（创建者），可动态创建
- **Group**：P2P 为 2 人群，多人群为 group
- **Message**：归属于 group，有 `senderId`（可能是 human 或 agent）

### 1.3 消息路由与唤醒

- **store.sendDirectMessage**：创建/复用 P2P 群，写入消息，返回 `groupId`、`messageId`
- **store.sendMessage**：向群发消息
- **wakeAgent(agentId)**：通知目标 agent 有新消息，触发其 ReAct 循环
- **ensureRunner(agentId)**：确保目标 agent 有运行中的 Runner 实例

### 1.4 事件总线（AgentEventBus）

- 按 `agentId` 分 channel
- 事件类型：`agent.wakeup`、`agent.unread`、`agent.stream`、`agent.done`、`agent.error`
- 前端通过 SSE 订阅 `/api/agents/:agentId/context-stream` 获取 agent 推理流

### 1.5 UI 总线（WorkspaceUIBus）

- 按 `workspaceId` 分 channel
- 事件：`ui.agent.created`、`ui.group.created`、`ui.message.created`、`ui.db.write`
- 用于 Graph 拓扑更新、对话列表刷新

---

## 二、Bee 当前架构对比

| 维度 | Bee | swarm-ide |
|------|-----|-----------|
| Agent | 固定 assistants.toml | 动态 create |
| 会话 | session_id + assistant_id | workspace + group + agent |
| 消息 | ContextManager 内存 | DB 持久化（groups/messages） |
| 1 对 1 | 用户 ↔ 单助手 | 用户 ↔ 任意 agent，agent ↔ agent |
| 群聊 | 无 | create_group + send_group_message |
| 拓扑 | 无 | Graph 实时展示 |
| 存储 | SQLite/JSON | PostgreSQL |

---

## 三、Bee 参考实施路线图

### Phase 1：群聊会话（不引入动态 Agent）

**目标**：用户可创建群聊，选多个 assistant 参与，1 条用户消息 → 多个 assistant 各自回复。

| 步骤 | 内容 | 预估 |
|------|------|------|
| 1.1 | 新增 `Group` 概念：`group_id`，成员为 `[assistant_id, ...]`，可选包含 `human` | 2h |
| 1.2 | 会话 key 扩展：`(session_id, group_id)` 或 `group_id` 即 session | 2h |
| 1.3 | API：`POST /api/groups` 创建群，`POST /api/chat/stream` 支持 `group_id` | 3h |
| 1.4 | 群聊消息流：用户发消息 → 并行/串行调用各 assistant 的 ReAct，汇聚回复 | 4h |
| 1.5 | 前端：群聊创建 UI、消息按 assistant 展示 | 3h |

**数据模型（SQLite 或 JSON）**：

```rust
// 可选：若用 SQLite
struct Group {
    id: String,
    member_assistant_ids: Vec<String>,  // 不含 human，human 隐式参与
    name: Option<String>,
    created_at: DateTime,
}
struct GroupMessage {
    id: String,
    group_id: String,
    sender: Sender,  // Human | Assistant(id)
    content: String,
    created_at: DateTime,
}
```

### Phase 2：Agent 间通信（send 工具）

**目标**：assistant 可调用 `send` 工具向另一个 assistant 发消息，形成 agent↔agent 对话。

| 步骤 | 内容 | 预估 |
|------|------|------|
| 2.1 | 新增 Tool：`send`，参数 `{ to: assistant_id, content: string }` | 2h |
| 2.2 | 实现：创建/复用 P2P 群（assistant_a, assistant_b），写入消息 | 2h |
| 2.3 | 目标 assistant 的「收件箱」：未读消息触发其 ReAct（需后台 runner） | 4h |
| 2.4 | 前端：支持查看 assistant↔assistant 的会话（可选） | 2h |

**与 swarm-ide 的差异**：Bee 的 assistant 来自配置，不动态 create；send 的 `to` 限定为已有 assistant_id。

### Phase 3：动态 create sub-agent（可选，较大改动）

**目标**：assistant 可调用 `create` 工具创建 sub-agent，形成树状拓扑。

| 步骤 | 内容 | 预估 |
|------|------|------|
| 3.1 | 新增 Tool：`create`，参数 `{ role, guidance }` | 1h |
| 3.2 | 运行时 agent 注册表：`HashMap<agent_id, (role, parent_id, context)>` | 3h |
| 3.3 | 新 agent 的 system prompt 由 role + guidance 生成，可复用现有 Planner | 2h |
| 3.4 | 持久化：agents 表 + 与 session 的关联 | 3h |
| 3.5 | 前端：Graph 展示 agent 树，点击可介入任意 agent 对话 | 6h |

**数据模型**：

```rust
struct Agent {
    id: String,
    workspace_id: String,
    role: String,
    parent_id: Option<String>,
    guidance: Option<String>,
    created_at: DateTime,
}
```

### Phase 4：Graph 拓扑与 Event 流（参考 swarm-ide）

**目标**：实时展示蜂群拓扑与通信链路。

| 步骤 | 内容 | 预估 |
|------|------|------|
| 4.1 | 后端：WorkspaceEventBus，emit `agent.created`、`group.created`、`message.created` | 2h |
| 4.2 | SSE：`/api/workspaces/:id/events` 推送拓扑变更 | 2h |
| 4.3 | 前端：使用 D3/vis.js 等渲染 Graph，节点=agent，边=message 流向 | 6h |
| 4.4 | Agent 推理流：`/api/agents/:id/context-stream`，展示 LLM 思考过程 | 3h |

---

## 四、推荐实施顺序

1. **Phase 1**（群聊）：收益高、改动相对可控，建议优先
2. **Phase 2**（send 工具）：依赖群聊与消息持久化，可作为 Phase 1 的延伸
3. **Phase 4**（Graph/Event）：可独立于 3，用于增强可观测性
4. **Phase 3**（动态 create）：改动最大，可视需求延后或裁剪

---

## 五、关键代码参考（swarm-ide）

| 功能 | 文件 | 要点 |
|------|------|------|
| create 工具 | `backend/src/runtime/agent-runtime.ts` L713-738 | `store.createSubAgentWithP2P`，`ensureRunner`，`getWorkspaceUIBus().emit` |
| send 工具 | `backend/src/runtime/agent-runtime.ts` L743-788 | `store.sendDirectMessage`，`wakeAgent(to)` |
| send_group_message | L861-914 | `store.sendMessage`，遍历 members 唤醒 agent |
| 消息存储 | `backend/src/lib/storage.ts` | `createGroup`，`sendMessage`，`sendDirectMessage` |
| 事件总线 | `backend/src/runtime/event-bus.ts` | 按 agentId 分 channel，emit/subscribe |
| 数据模型 | `backend/src/db/schema.ts` | agents, groups, groupMembers, messages |

---

## 六、与 Bee 现有模块的映射

| swarm-ide | Bee 对应 |
|-----------|----------|
| store (storage.ts) | 扩展 `memory` 或新增 `swarm` 模块 |
| AgentRunner | 扩展 `react::react_loop`，支持「被唤醒」模式 |
| create/send 等工具 | 新增 `tools::swarm.rs`，注册到 ToolRegistry |
| WorkspaceUIBus | 可复用 gateway 的 broadcast，或新增 `event_bus` |
| Graph 前端 | 新增 `static/swarm.html` 或集成到 index.html |

---

*文档版本：v1.0，基于 swarm-ide chore/specs-mvp 分支*
