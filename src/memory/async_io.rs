//! 异步文件 I/O（解决问题 2.2）
//!
//! 提供记忆文件的异步读写操作，避免在 async 上下文中阻塞。

use std::path::Path;

use tokio::fs;
use tokio::io::AsyncWriteExt;

/// 异步读取 lessons 内容
pub async fn load_lessons_async(path: &Path) -> String {
    match fs::read_to_string(path).await {
        Ok(s) => s.trim().to_string(),
        Err(_) => String::new(),
    }
}

/// 异步读取 procedural 内容
pub async fn load_procedural_async(path: &Path) -> String {
    match fs::read_to_string(path).await {
        Ok(s) => s.trim().to_string(),
        Err(_) => String::new(),
    }
}

/// 异步读取 preferences 内容
pub async fn load_preferences_async(path: &Path) -> String {
    match fs::read_to_string(path).await {
        Ok(s) => s.trim().to_string(),
        Err(_) => String::new(),
    }
}

/// 异步追加用户偏好
pub async fn append_preference_async(path: &Path, content: &str) -> std::io::Result<()> {
    if content.trim().is_empty() {
        return Ok(());
    }
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).await?;
    }
    let line = format!("- {}\n", content.trim());
    
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(line.as_bytes()).await
}

/// 异步追加教训
pub async fn append_lesson_async(path: &Path, line: &str) -> std::io::Result<()> {
    if line.trim().is_empty() {
        return Ok(());
    }
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).await?;
    }
    let content = format!("{}\n", line.trim());
    
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(content.as_bytes()).await
}

/// 异步追加程序记忆
pub async fn append_procedural_async(
    path: &Path,
    tool: &str,
    success: bool,
    detail: &str,
) -> std::io::Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).await?;
    }
    let status = if success { "ok" } else { "fail" };
    let line = format!("- {} {}: {}\n", tool, status, detail);
    
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(line.as_bytes()).await
}

/// 异步追加心跳日志
pub async fn append_heartbeat_log_async(path: &Path, reply: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let line = format!(
        "\n## {}\n\n{}\n\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        reply.trim()
    );
    
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(line.as_bytes()).await
}

/// 异步追加每日日志
pub async fn append_daily_log_async(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(content.as_bytes()).await
}

/// 异步读取文件内容
pub async fn read_file_async(path: &Path) -> std::io::Result<String> {
    fs::read_to_string(path).await
}

/// 异步写入文件内容
pub async fn write_file_async(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(path, content).await
}

/// 异步检查文件是否存在
pub async fn file_exists_async(path: &Path) -> bool {
    fs::metadata(path).await.is_ok()
}

/// 在 spawn_blocking 中执行同步 I/O（用于无法异步化的场景）
pub async fn blocking_read(path: impl AsRef<Path> + Send + 'static) -> std::io::Result<String> {
    tokio::task::spawn_blocking(move || std::fs::read_to_string(path.as_ref()))
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
}

/// 在 spawn_blocking 中执行同步写入
pub async fn blocking_write(
    path: impl AsRef<Path> + Send + 'static,
    content: String,
) -> std::io::Result<()> {
    tokio::task::spawn_blocking(move || std::fs::write(path.as_ref(), content))
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_load_lessons_async() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("lessons.md");

            // 文件不存在时返回空字符串
            let content = load_lessons_async(&path).await;
            assert!(content.is_empty());

            // 写入内容后能读取
            fs::write(&path, "Test lesson").await.unwrap();
            let content = load_lessons_async(&path).await;
            assert_eq!(content, "Test lesson");
        });
    }

    #[test]
    fn test_append_lesson_async() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("lessons.md");

            append_lesson_async(&path, "Lesson 1").await.unwrap();
            append_lesson_async(&path, "Lesson 2").await.unwrap();

            let content = fs::read_to_string(&path).await.unwrap();
            assert!(content.contains("Lesson 1"));
            assert!(content.contains("Lesson 2"));
        });
    }

    #[test]
    fn test_append_procedural_async() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("procedural.md");

            append_procedural_async(&path, "shell", true, "executed successfully").await.unwrap();
            append_procedural_async(&path, "cat", false, "file not found").await.unwrap();

            let content = fs::read_to_string(&path).await.unwrap();
            assert!(content.contains("shell ok"));
            assert!(content.contains("cat fail"));
        });
    }

    #[test]
    fn test_file_exists_async() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = TempDir::new().unwrap();
            let path = dir.path().join("test.txt");

            assert!(!file_exists_async(&path).await);

            fs::write(&path, "test").await.unwrap();
            assert!(file_exists_async(&path).await);
        });
    }
}
