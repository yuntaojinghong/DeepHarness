//! 统一错误类型。
//!
//! 所有面向 IPC 的错误都实现 `Serialize`，以便前端以字符串形式收到
//! 完整、可读的错误信息；内部错误链通过 `#[from]` 保留原始来源。

use serde::Serialize;
use thiserror::Error;

/// 应用统一错误类型。
#[derive(Debug, Error)]
pub enum AppError {
    /// 请求的路径不在该 Agent 的授权白名单内。
    #[error("路径不在授权范围内: {0}")]
    PathNotAuthorized(String),

    /// 请求的路径不存在。
    #[error("路径不存在: {0}")]
    PathNotFound(String),

    /// 未知或非法的 Agent 标识。
    #[error("未知 Agent: {0}")]
    UnknownAgent(String),

    /// 文件系统 IO 错误。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    /// JSON / 配置解析错误。
    #[error("配置解析错误: {0}")]
    Config(#[from] serde_json::Error),

    /// 权限存储损坏或无法解析。
    #[error("权限存储错误: {0}")]
    PermissionStore(String),

    /// 其他错误。
    #[error("{0}")]
    Other(String),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// 应用统一 Result 别名。
pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_converts_into_app_error() {
        let err: AppError = std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file missing",
        )
        .into();
        assert!(matches!(err, AppError::Io(_)));
        assert!(err.to_string().contains("file missing"));
    }

    #[test]
    fn serializes_to_readable_string() {
        let err = AppError::PathNotAuthorized("C:\\Windows\\secret".to_string());
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("路径不在授权范围内"));
    }
}
