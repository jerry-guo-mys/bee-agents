//! 技能系统
//!
//! 技能（Skill）是一组能力描述、模板和脚本的集合。
//! 助手可以按需从技能库中选择相关技能，动态增强其能力。
//!
//! 目录结构：
//! ```text
//! config/skills/
//! ├── viral/
//! │   ├── skill.toml      # 技能元数据
//! │   ├── capability.md   # 能力描述（用于 LLM 选择）
//! │   ├── template.md     # 模板（可选）
//! │   └── script.py       # 脚本（可选）
//! └── ...
//! ```

mod loader;
mod selector;

pub use loader::{Skill, SkillCache, SkillLoader};
pub use selector::SkillSelector;
