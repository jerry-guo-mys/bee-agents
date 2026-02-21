//! 技能加载器
//!
//! 从 config/skills/ 目录加载技能并缓存。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use tokio::sync::RwLock;

/// 技能元数据（skill.toml）
#[derive(Debug, Clone, Deserialize)]
pub struct SkillMeta {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default)]
    pub script_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SkillToml {
    skill: SkillMeta,
}

/// 完整技能数据
#[derive(Debug, Clone)]
pub struct Skill {
    pub meta: SkillMeta,
    pub capability: String,
    pub template: Option<String>,
    pub script_path: Option<PathBuf>,
    pub dir: PathBuf,
}

impl Skill {
    /// 获取用于 LLM 选择的简短描述
    pub fn summary(&self) -> String {
        format!(
            "[{}] {}: {}",
            self.meta.id, self.meta.name, self.meta.description
        )
    }

    /// 获取完整的能力描述（包含 capability.md 内容）
    pub fn full_capability(&self) -> String {
        format!(
            "# {} ({})\n\n{}\n\n{}",
            self.meta.name,
            self.meta.id,
            self.meta.description,
            self.capability
        )
    }

    /// 获取模板内容（如果有）
    pub fn get_template(&self) -> Option<&str> {
        self.template.as_deref()
    }
}

/// 技能缓存
pub type SkillCache = Arc<RwLock<HashMap<String, Skill>>>;

/// 技能加载器
pub struct SkillLoader {
    skills_dir: PathBuf,
    cache: SkillCache,
}

impl SkillLoader {
    /// 创建新的加载器
    pub fn new(skills_dir: impl AsRef<Path>) -> Self {
        Self {
            skills_dir: skills_dir.as_ref().to_path_buf(),
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 从默认位置创建
    pub fn from_default() -> Self {
        let dirs = [
            PathBuf::from("config/skills"),
            PathBuf::from("../config/skills"),
        ];
        let skills_dir = dirs
            .into_iter()
            .find(|d| d.exists())
            .unwrap_or_else(|| PathBuf::from("config/skills"));
        Self::new(skills_dir)
    }

    /// 获取缓存引用
    pub fn cache(&self) -> SkillCache {
        Arc::clone(&self.cache)
    }

    /// 加载所有技能并缓存
    pub async fn load_all(&self) -> anyhow::Result<Vec<Skill>> {
        let mut skills = Vec::new();

        if !self.skills_dir.exists() {
            return Ok(skills);
        }

        let entries = std::fs::read_dir(&self.skills_dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(skill) = self.load_skill(&path) {
                    skills.push(skill);
                }
            }
        }

        let mut cache = self.cache.write().await;
        for skill in &skills {
            cache.insert(skill.meta.id.clone(), skill.clone());
        }

        tracing::info!("Loaded {} skills", skills.len());
        Ok(skills)
    }

    /// 加载单个技能
    fn load_skill(&self, dir: &Path) -> Option<Skill> {
        let skill_toml = dir.join("skill.toml");
        if !skill_toml.exists() {
            return None;
        }

        let toml_content = std::fs::read_to_string(&skill_toml).ok()?;
        let skill_data: SkillToml = toml::from_str(&toml_content).ok()?;
        let meta = skill_data.skill;

        let capability_path = dir.join("capability.md");
        let capability = std::fs::read_to_string(&capability_path).unwrap_or_default();

        let template_path = dir.join("template.md");
        let template = std::fs::read_to_string(&template_path).ok();

        let script_path = meta.script.as_ref().map(|s| dir.join(s));

        Some(Skill {
            meta,
            capability,
            template,
            script_path,
            dir: dir.to_path_buf(),
        })
    }

    /// 根据 ID 获取技能
    pub async fn get(&self, id: &str) -> Option<Skill> {
        let cache = self.cache.read().await;
        cache.get(id).cloned()
    }

    /// 获取所有技能的摘要列表
    pub async fn list_summaries(&self) -> Vec<String> {
        let cache = self.cache.read().await;
        cache.values().map(|s| s.summary()).collect()
    }

    /// 获取所有技能 ID
    pub async fn list_ids(&self) -> Vec<String> {
        let cache = self.cache.read().await;
        cache.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_summary() {
        let skill = Skill {
            meta: SkillMeta {
                id: "test".to_string(),
                name: "测试技能".to_string(),
                description: "这是一个测试技能".to_string(),
                tags: vec![],
                script: None,
                script_type: None,
            },
            capability: "# 能力\n测试能力描述".to_string(),
            template: None,
            script_path: None,
            dir: PathBuf::from("."),
        };

        assert!(skill.summary().contains("test"));
        assert!(skill.summary().contains("测试技能"));
    }
}
