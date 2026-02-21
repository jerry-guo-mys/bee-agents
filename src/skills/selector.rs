//! 技能选择器
//!
//! 根据用户查询从缓存的技能描述中选择相关技能。

use std::sync::Arc;

use crate::llm::LlmClient;
use crate::memory::Message;

use super::{Skill, SkillCache};

/// 技能选择器
pub struct SkillSelector {
    cache: SkillCache,
    llm: Arc<dyn LlmClient>,
    max_skills: usize,
}

impl SkillSelector {
    /// 创建新的选择器
    pub fn new(cache: SkillCache, llm: Arc<dyn LlmClient>) -> Self {
        Self {
            cache,
            llm,
            max_skills: 3,
        }
    }

    /// 设置最大选择技能数
    pub fn with_max_skills(mut self, max: usize) -> Self {
        self.max_skills = max;
        self
    }

    /// 根据用户查询选择相关技能
    pub async fn select(&self, query: &str) -> Vec<Skill> {
        let cache = self.cache.read().await;
        let skills: Vec<&Skill> = cache.values().collect();

        if skills.is_empty() {
            return vec![];
        }

        if skills.len() <= self.max_skills {
            return skills.into_iter().cloned().collect();
        }

        let summaries: Vec<String> = skills.iter().map(|s| s.summary()).collect();
        let skill_list = summaries.join("\n");

        let system = format!(
            "You are a skill selector. Given the user's query and available skills, select the most relevant skills.\n\
             Reply with ONLY the skill IDs (comma-separated, max {} skills). No explanation.\n\n\
             Available skills:\n{}",
            self.max_skills, skill_list
        );

        let user_msg = format!("User query: {}", query);

        let messages = vec![
            Message::system(system),
            Message::user(user_msg),
        ];

        let result = self
            .llm
            .complete(&messages)
            .await
            .unwrap_or_default();

        let selected_ids: Vec<&str> = result
            .trim()
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .take(self.max_skills)
            .collect();

        let mut selected = Vec::new();
        for id in selected_ids {
            let id_lower = id.to_lowercase();
            if let Some(skill) = skills.iter().find(|s| {
                s.meta.id.to_lowercase() == id_lower
                    || s.meta.id.to_lowercase().contains(&id_lower)
            }) {
                selected.push((*skill).clone());
            }
        }

        if selected.is_empty() && !skills.is_empty() {
            selected.push(skills[0].clone());
        }

        selected
    }

    /// 根据标签快速筛选（不调用 LLM）
    pub async fn filter_by_tags(&self, tags: &[&str]) -> Vec<Skill> {
        let cache = self.cache.read().await;
        cache
            .values()
            .filter(|s| {
                tags.iter().any(|t| {
                    s.meta
                        .tags
                        .iter()
                        .any(|st| st.to_lowercase().contains(&t.to_lowercase()))
                })
            })
            .cloned()
            .collect()
    }

    /// 根据 ID 列表获取技能
    pub async fn get_by_ids(&self, ids: &[&str]) -> Vec<Skill> {
        let cache = self.cache.read().await;
        ids.iter()
            .filter_map(|id| cache.get(*id).cloned())
            .collect()
    }

    /// 生成选中技能的增强 prompt
    pub fn build_skills_prompt(skills: &[Skill]) -> String {
        if skills.is_empty() {
            return String::new();
        }

        let mut parts = vec!["## 可用技能\n".to_string()];

        for skill in skills {
            parts.push(format!("### {}\n", skill.meta.name));
            parts.push(skill.capability.clone());
            parts.push("\n".to_string());

            if let Some(template) = &skill.template {
                parts.push(format!("#### 模板\n{}\n\n", template));
            }
        }

        parts.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_skills_prompt() {
        use super::super::loader::SkillMeta;
        use std::path::PathBuf;

        let skills = vec![Skill {
            meta: SkillMeta {
                id: "test".to_string(),
                name: "测试".to_string(),
                description: "测试描述".to_string(),
                tags: vec![],
                script: None,
                script_type: None,
            },
            capability: "能力描述".to_string(),
            template: Some("模板内容".to_string()),
            script_path: None,
            dir: PathBuf::from("."),
        }];

        let prompt = SkillSelector::build_skills_prompt(&skills);
        assert!(prompt.contains("测试"));
        assert!(prompt.contains("能力描述"));
        assert!(prompt.contains("模板内容"));
    }
}
