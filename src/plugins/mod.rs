//! Plugin 系统（Phase 4 长期演进）
//!
//! 提供标准化的插件接口，支持动态加载和扩展工具能力。
//! 
//! 插件类型：
//! - 工具插件：扩展可用工具
//! - 提供者插件：扩展 LLM/嵌入提供者
//! - 处理器插件：消息预处理/后处理

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 插件元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// 插件唯一标识
    pub id: String,
    /// 插件名称
    pub name: String,
    /// 版本号
    pub version: String,
    /// 描述
    pub description: String,
    /// 作者
    pub author: Option<String>,
    /// 插件类型
    pub plugin_type: PluginType,
    /// 依赖的其他插件
    pub dependencies: Vec<String>,
}

impl PluginMetadata {
    pub fn new(id: impl Into<String>, name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version: version.into(),
            description: String::new(),
            author: None,
            plugin_type: PluginType::Tool,
            dependencies: Vec::new(),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    pub fn with_type(mut self, plugin_type: PluginType) -> Self {
        self.plugin_type = plugin_type;
        self
    }
}

/// 插件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginType {
    /// 工具插件
    Tool,
    /// LLM 提供者插件
    LlmProvider,
    /// 嵌入提供者插件
    EmbeddingProvider,
    /// 消息处理器插件
    MessageProcessor,
    /// 存储插件
    Storage,
}

/// 插件状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    /// 已注册但未初始化
    Registered,
    /// 已初始化
    Initialized,
    /// 已启用
    Enabled,
    /// 已禁用
    Disabled,
    /// 出错
    Error,
}

/// 插件上下文（提供给插件访问系统资源）
pub struct PluginContext {
    /// 配置
    pub config: HashMap<String, Value>,
    /// 工作目录
    pub workspace: std::path::PathBuf,
}

impl PluginContext {
    pub fn new(workspace: impl Into<std::path::PathBuf>) -> Self {
        Self {
            config: HashMap::new(),
            workspace: workspace.into(),
        }
    }

    pub fn with_config(mut self, key: impl Into<String>, value: Value) -> Self {
        self.config.insert(key.into(), value);
        self
    }

    pub fn get_config<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.config.get(key).and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

/// 插件 trait
#[async_trait]
pub trait Plugin: Send + Sync {
    /// 获取插件元数据
    fn metadata(&self) -> &PluginMetadata;

    /// 初始化插件
    async fn initialize(&mut self, ctx: &PluginContext) -> Result<(), PluginError>;

    /// 启用插件
    async fn enable(&mut self) -> Result<(), PluginError> {
        Ok(())
    }

    /// 禁用插件
    async fn disable(&mut self) -> Result<(), PluginError> {
        Ok(())
    }

    /// 关闭插件（清理资源）
    async fn shutdown(&mut self) -> Result<(), PluginError> {
        Ok(())
    }

    /// 获取插件状态
    fn state(&self) -> PluginState;

    /// 转换为 Any（用于向下转型）
    fn as_any(&self) -> &dyn Any;
    
    /// 转换为可变 Any
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// 工具插件 trait
#[async_trait]
pub trait ToolPlugin: Plugin {
    /// 获取工具名称
    fn tool_name(&self) -> &str;

    /// 获取工具描述
    fn tool_description(&self) -> &str;

    /// 获取参数 schema
    fn parameters_schema(&self) -> Value;

    /// 执行工具
    async fn execute(&self, args: Value) -> Result<String, PluginError>;
}

/// 消息处理器插件 trait
#[async_trait]
pub trait MessageProcessorPlugin: Plugin {
    /// 预处理用户消息
    async fn preprocess(&self, message: &str) -> Result<String, PluginError> {
        Ok(message.to_string())
    }

    /// 后处理助手回复
    async fn postprocess(&self, response: &str) -> Result<String, PluginError> {
        Ok(response.to_string())
    }
}

/// 插件错误
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("Plugin not found: {0}")]
    NotFound(String),

    #[error("Plugin initialization failed: {0}")]
    InitializationFailed(String),

    #[error("Plugin execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Plugin configuration error: {0}")]
    ConfigError(String),

    #[error("Plugin dependency not satisfied: {0}")]
    DependencyError(String),

    #[error("Plugin already registered: {0}")]
    AlreadyRegistered(String),

    #[error("Invalid plugin state: expected {expected:?}, got {actual:?}")]
    InvalidState {
        expected: PluginState,
        actual: PluginState,
    },
}

/// 插件注册表
pub struct PluginRegistry {
    plugins: HashMap<String, Arc<tokio::sync::RwLock<Box<dyn Plugin>>>>,
    tool_plugins: HashMap<String, Arc<tokio::sync::RwLock<Box<dyn ToolPlugin>>>>,
    processor_plugins: Vec<Arc<tokio::sync::RwLock<Box<dyn MessageProcessorPlugin>>>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            tool_plugins: HashMap::new(),
            processor_plugins: Vec::new(),
        }
    }

    /// 注册插件
    pub fn register(&mut self, plugin: Box<dyn Plugin>) -> Result<(), PluginError> {
        let id = plugin.metadata().id.clone();
        if self.plugins.contains_key(&id) {
            return Err(PluginError::AlreadyRegistered(id));
        }
        self.plugins.insert(id, Arc::new(tokio::sync::RwLock::new(plugin)));
        Ok(())
    }

    /// 注册工具插件
    pub fn register_tool(&mut self, plugin: Box<dyn ToolPlugin>) -> Result<(), PluginError> {
        let id = plugin.metadata().id.clone();
        let tool_name = plugin.tool_name().to_string();
        
        if self.tool_plugins.contains_key(&tool_name) {
            return Err(PluginError::AlreadyRegistered(tool_name));
        }
        
        self.tool_plugins.insert(tool_name, Arc::new(tokio::sync::RwLock::new(plugin)));
        
        // 同时注册到通用插件表
        // 由于所有权问题，这里需要重新创建
        tracing::debug!("Registered tool plugin: {}", id);
        Ok(())
    }

    /// 注册消息处理器插件
    pub fn register_processor(&mut self, plugin: Box<dyn MessageProcessorPlugin>) {
        self.processor_plugins.push(Arc::new(tokio::sync::RwLock::new(plugin)));
    }

    /// 初始化所有插件
    pub async fn initialize_all(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        for (id, plugin) in &self.plugins {
            let mut plugin = plugin.write().await;
            plugin.initialize(ctx).await.map_err(|e| {
                tracing::error!("Failed to initialize plugin {}: {}", id, e);
                e
            })?;
        }
        
        for (name, plugin) in &self.tool_plugins {
            let mut plugin = plugin.write().await;
            plugin.initialize(ctx).await.map_err(|e| {
                tracing::error!("Failed to initialize tool plugin {}: {}", name, e);
                e
            })?;
        }
        
        for plugin in &self.processor_plugins {
            let mut plugin = plugin.write().await;
            plugin.initialize(ctx).await?;
        }
        
        Ok(())
    }

    /// 获取工具插件
    pub fn get_tool(&self, name: &str) -> Option<Arc<tokio::sync::RwLock<Box<dyn ToolPlugin>>>> {
        self.tool_plugins.get(name).cloned()
    }

    /// 列出所有工具插件
    pub fn list_tools(&self) -> Vec<String> {
        self.tool_plugins.keys().cloned().collect()
    }

    /// 执行工具插件
    pub async fn execute_tool(&self, name: &str, args: Value) -> Result<String, PluginError> {
        let plugin = self
            .tool_plugins
            .get(name)
            .ok_or_else(|| PluginError::NotFound(name.to_string()))?;
        
        let plugin = plugin.read().await;
        plugin.execute(args).await
    }

    /// 预处理消息（通过所有处理器）
    pub async fn preprocess_message(&self, message: &str) -> Result<String, PluginError> {
        let mut result = message.to_string();
        for plugin in &self.processor_plugins {
            let plugin = plugin.read().await;
            result = plugin.preprocess(&result).await?;
        }
        Ok(result)
    }

    /// 后处理响应（通过所有处理器）
    pub async fn postprocess_response(&self, response: &str) -> Result<String, PluginError> {
        let mut result = response.to_string();
        for plugin in &self.processor_plugins {
            let plugin = plugin.read().await;
            result = plugin.postprocess(&result).await?;
        }
        Ok(result)
    }

    /// 关闭所有插件
    pub async fn shutdown_all(&self) -> Result<(), PluginError> {
        for (_, plugin) in &self.plugins {
            let mut plugin = plugin.write().await;
            plugin.shutdown().await?;
        }
        
        for (_, plugin) in &self.tool_plugins {
            let mut plugin = plugin.write().await;
            plugin.shutdown().await?;
        }
        
        for plugin in &self.processor_plugins {
            let mut plugin = plugin.write().await;
            plugin.shutdown().await?;
        }
        
        Ok(())
    }

    /// 获取插件数量
    pub fn len(&self) -> usize {
        self.plugins.len() + self.tool_plugins.len() + self.processor_plugins.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 工具插件适配器（将 ToolPlugin 包装为 Tool trait）
pub struct ToolPluginAdapter {
    plugin: Arc<tokio::sync::RwLock<Box<dyn ToolPlugin>>>,
}

impl ToolPluginAdapter {
    pub fn new(plugin: Arc<tokio::sync::RwLock<Box<dyn ToolPlugin>>>) -> Self {
        Self { plugin }
    }
}

#[async_trait]
impl crate::tools::Tool for ToolPluginAdapter {
    fn name(&self) -> &str {
        // 由于需要 &str，这里返回静态字符串
        // 实际使用时需要 blocking 获取
        "plugin_tool"
    }

    fn description(&self) -> &str {
        "Plugin-based tool"
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({})
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let plugin = self.plugin.read().await;
        plugin.execute(args).await.map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPlugin {
        metadata: PluginMetadata,
        state: PluginState,
    }

    impl TestPlugin {
        fn new(id: &str) -> Self {
            Self {
                metadata: PluginMetadata::new(id, "Test Plugin", "1.0.0"),
                state: PluginState::Registered,
            }
        }
    }

    #[async_trait]
    impl Plugin for TestPlugin {
        fn metadata(&self) -> &PluginMetadata {
            &self.metadata
        }

        async fn initialize(&mut self, _ctx: &PluginContext) -> Result<(), PluginError> {
            self.state = PluginState::Initialized;
            Ok(())
        }

        fn state(&self) -> PluginState {
            self.state
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn test_plugin_metadata() {
        let meta = PluginMetadata::new("test", "Test", "1.0.0")
            .with_description("A test plugin")
            .with_author("Test Author")
            .with_type(PluginType::Tool);

        assert_eq!(meta.id, "test");
        assert_eq!(meta.name, "Test");
        assert_eq!(meta.plugin_type, PluginType::Tool);
    }

    #[test]
    fn test_plugin_registry() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut registry = PluginRegistry::new();
            
            let plugin = Box::new(TestPlugin::new("test1"));
            registry.register(plugin).unwrap();
            
            assert_eq!(registry.len(), 1);
            
            // 重复注册应失败
            let plugin2 = Box::new(TestPlugin::new("test1"));
            assert!(registry.register(plugin2).is_err());
        });
    }

    #[test]
    fn test_plugin_context() {
        let ctx = PluginContext::new("/tmp")
            .with_config("key1", serde_json::json!("value1"))
            .with_config("key2", serde_json::json!(42));

        assert_eq!(ctx.get_config::<String>("key1"), Some("value1".to_string()));
        assert_eq!(ctx.get_config::<i32>("key2"), Some(42));
        assert_eq!(ctx.get_config::<String>("missing"), None);
    }
}
