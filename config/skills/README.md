# 按文件扩展技能（Skills）

在此目录下每个 `*.toml` 文件可定义**一个**助手，无需修改 `config/assistants.toml`。

## 格式

```toml
[assistant]
id = "唯一ID"
name = "展示名称"
description = "简短描述，用于首页卡片"
prompt = "prompts/assistant-xxx.md"
```

- `prompt` 路径相对 `config/`，对应 `config/prompts/` 下的文件。
- 与 `assistants.toml` 中同 `id` 时，以本目录为准。

## 示例

见 `_example.toml`（以 `_` 开头不会参与加载，仅作参考可复制为 `xxx.toml` 使用）。

详细说明见项目根目录 **`docs/SKILLS.md`**。
