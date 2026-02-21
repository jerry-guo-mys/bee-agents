# 如何扩展 Bee 的 Skills（技能 / 助手）

Bee 里「扩展能力」主要有两类：**助手（Assistants）** 和 **工具插件（Tool Plugins）**。下面用最少改动、最易复制的方式说明如何扩展。

---

## 一、扩展助手（多助手 / 技能人格）

助手 = 一套 system prompt，决定 AI 的角色与风格（如自媒体助手、提分助手）。扩展后会在 Web 首页「选择助手」里出现。

### 方式 A：改主列表（适合集中管理）

编辑 **`config/assistants.toml`**，在 `[[assistants]]` 下增加一条：

```toml
[[assistants]]
id = "写作"
name = "英语写作助手"
description = "润色、扩写、纠错，适合作文与邮件"
prompt = "prompts/assistant-writing.txt"
```

然后在 **`config/prompts/`** 下新增 `assistant-writing.md`，写清该助手的身份与规则（可参考现有 `assistant-media.md` 等）。  
提示：`prompt` 路径相对 `config/`，且会**自动拼接工具调用 schema**，无需在 prompt 里再写一遍。

### 方式 B：按文件扩展（推荐，易分享）

**不用改 `assistants.toml`**，在 **`config/skills/`** 下新增一个 toml 文件即可，一个文件 = 一个技能。

1. 确保存在目录：`config/skills/`
2. 新建例如 **`config/skills/writing.toml`**：

```toml
[assistant]
id = "writing"
name = "英语写作助手"
description = "润色、扩写、纠错，适合作文与邮件"
prompt = "prompts/assistant-writing.txt"
```

3. 在 **`config/prompts/`** 下新增 **`assistant-writing.md`**，内容为该助手的 system 说明。

**规则**：

- 若某 `id` 在 `assistants.toml` 和 `config/skills/*.toml` 里都出现，**以 skills 目录里的为准**（便于用单文件覆盖/定制）。
- 只扫描 `config/skills/*.toml`，子目录不会递归。

这样扩展技能 = **新增一个 toml + 一个 prompt 文件**，无需改 Rust 或主配置列表。

---

## 二、扩展工具（让 AI 能调新能力）

工具插件 = 通过配置注册一个「可执行程序 + 参数模板」，AI 在 ReAct 里会按名称调用。

在 **`config/default.toml`** 的 **`[tools]`** 下增加 **`[[tools.plugins]]`**：

```toml
[[tools.plugins]]
name = "run_script"
description = "Run a Python script in workspace. Args: query (string)."
program = "python"
args = ["{{workspace}}/scripts/run.py", "{{query}}"]
# 可选：timeout_secs = 60
# 可选：working_dir = "scripts"
```

- **`name`**：工具名，LLM 看到的名称。
- **`description`**：给 LLM 看的说明，建议写明参数（如 `Args: query (string)`）。
- **`program`**：可执行程序（如 `python`、`node`、`/path/to/bin`）。
- **`args`**：参数列表；**`{{workspace}}`** 会替换为 Bee 工作区根路径，**`{{key}}`** 会从 LLM 传入的 JSON args 里取 `key` 的值。

执行时无 shell，直接 `program + args`，带超时；若需更复杂逻辑，可在脚本内完成。

---

## 三、推荐目录与命名约定

| 用途           | 位置                         | 说明 |
|----------------|------------------------------|------|
| 助手主列表     | `config/assistants.toml`     | 统一维护所有助手时可只改这里 |
| 单文件扩展技能 | `config/skills/*.toml`      | 一个 toml = 一个技能，易增删、易分享 |
| 助手 prompt    | `config/prompts/assistant-*.md` | 与 assistants/skills 里的 `prompt` 字段对应 |
| 通用 system    | `config/prompts/system.md`  | 默认助手的 system prompt |
| 工具插件       | `config/default.toml` → `[[tools.plugins]]` | 动态工具，无需改代码 |

---

## 四、小结：怎样「更容易」扩展

- **只加新助手/技能**：在 **`config/skills/`** 加一个 **`xxx.toml`**（`[assistant]` 表） + **`config/prompts/assistant-xxx.md`**，重启 bee-web 即可。
- **只加新工具**：在 **`config/default.toml`** 加一段 **`[[tools.plugins]]`**，写好 `name/description/program/args`，重启即可。
- 两者可组合：新助手用现有工具；新工具对所有助手生效（由同一 ToolRegistry 注册）。

这样扩展 skills = **改配置 + 加 prompt 或脚本**，无需改 Rust 代码。
