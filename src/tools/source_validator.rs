use async_trait::async_trait;
use serde_json::Value;

use crate::tools::Tool;

pub struct SourceValidatorTool {
    trusted_domains: Vec<String>,
}

impl SourceValidatorTool {
    pub fn new(trusted_domains: Vec<String>) -> Self {
        Self { trusted_domains }
    }

    fn calculate_trust_score(&self, url: &str) -> f32 {
        let domain = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .unwrap_or(url)
            .split('/')
            .next()
            .unwrap_or("")
            .to_lowercase();

        if self.trusted_domains.iter().any(|d| domain.contains(d)) {
            return 0.9;
        }

        if domain.contains(".edu") || domain.contains(".gov") {
            return 0.85;
        }

        if domain.contains("wikipedia") {
            return 0.8;
        }

        if domain.contains("github.com") || domain.contains("stackoverflow.com") {
            return 0.75;
        }

        0.5
    }
}

#[async_trait]
impl Tool for SourceValidatorTool {
    fn name(&self) -> &str {
        "validate_source"
    }

    fn description(&self) -> &str {
        "Validate the credibility of a web source. Returns trust score (0-1) and credibility analysis. Args: {\"url\": \"https://...\", \"content\": \"optional content snippet\"}"
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if url.is_empty() {
            return Err("Missing url".to_string());
        }

        let trust_score = self.calculate_trust_score(url);
        
        let credibility = if trust_score >= 0.8 {
            "high"
        } else if trust_score >= 0.6 {
            "medium"
        } else {
            "low"
        };

        let output = serde_json::json!({
            "url": url,
            "trust_score": trust_score,
            "credibility": credibility,
            "recommendation": if trust_score >= 0.7 {
                "reliable source for research"
            } else {
                "use with caution, cross-reference with other sources"
            }
        });

        Ok(output.to_string())
    }
}
