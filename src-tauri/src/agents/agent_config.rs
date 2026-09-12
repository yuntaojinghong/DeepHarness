//! DeepHarness Native Agent 的模型配置持久化。
//!
//! 配置保存在该 Agent 隔离目录的 `config/agent_config.json`：
//! - 只存在于本机用户数据目录，绝不进入仓库、日志或版本控制；
//! - API Key 属于敏感数据：Worker 在 `configure` 时接收明文并仅驻留
//!   内存；前端读取配置接口不回显 Key 全文（见 registry 命令层）。
//!
//! 前端流程：设置页提交 baseUrl/apiKey/model → `deepharness_configure`
//! 保存并热注入 Worker；Worker 未运行时仅落盘，下次启动自动注入。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::paths::AgentDirs;

/// DeepHarness Native Agent 的模型接入配置。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelConfig {
    /// OpenAI 兼容 API 根地址（默认 DeepSeek 官方 https://api.deepseek.com）。
    pub base_url: String,
    /// API Key（敏感数据）。
    pub api_key: String,
    /// 模型名（如 deepseek-chat / deepseek-reasoner）。
    pub model: String,
}

impl AgentModelConfig {
    /// 校验三个字段在 trim 后均非空。
    pub fn validate(&self) -> AppResult<()> {
        if self.base_url.trim().is_empty() {
            return Err(AppError::Other("baseUrl 不能为空".to_string()));
        }
        if self.api_key.trim().is_empty() {
            return Err(AppError::Other("apiKey 不能为空".to_string()));
        }
        if self.model.trim().is_empty() {
            return Err(AppError::Other("model 不能为空".to_string()));
        }
        Ok(())
    }

    /// 配置文件路径：<agent>/config/agent_config.json。
    pub fn path(dirs: &AgentDirs) -> PathBuf {
        dirs.config.join("agent_config.json")
    }

    /// 保存配置（幂等覆盖；目录不存在时自动创建）。
    pub fn save(&self, dirs: &AgentDirs) -> AppResult<()> {
        self.validate()?;
        std::fs::create_dir_all(&dirs.config)?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Other(format!("配置序列化失败: {e}")))?;
        std::fs::write(Self::path(dirs), json)?;
        Ok(())
    }

    /// 读取配置；文件不存在或内容损坏时返回 `None`
    /// （调用方应提示用户重新配置，而不是让 Worker 拿到残缺参数）。
    pub fn load(dirs: &AgentDirs) -> Option<Self> {
        let raw = std::fs::read_to_string(Self::path(dirs)).ok()?;
        match serde_json::from_str(&raw) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                tracing::warn!("agent_config.json 解析失败（忽略并要求重新配置）: {e}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AgentDirs;

    fn dirs() -> AgentDirs {
        let root = std::env::temp_dir()
            .join(format!("dh_cfg_{}", std::process::id()))
            .join("agents")
            .join("deepharness");
        AgentDirs::from_root(root, "deepharness")
    }

    fn sample() -> AgentModelConfig {
        AgentModelConfig {
            base_url: "https://api.deepseek.com".to_string(),
            api_key: "sk-test-123".to_string(),
            model: "deepseek-chat".to_string(),
        }
    }

    #[test]
    fn validate_accepts_complete_config() {
        assert!(sample().validate().is_ok());
    }

    #[test]
    fn validate_rejects_empty_fields() {
        for field in ["base_url", "api_key", "model"] {
            let mut cfg = sample();
            match field {
                "base_url" => cfg.base_url = "  ".to_string(),
                "api_key" => cfg.api_key = String::new(),
                "model" => cfg.model = String::new(),
                _ => unreachable!(),
            }
            let err = cfg.validate().expect_err("空字段应被拒绝");
            assert!(err.to_string().contains("不能为空"), "{field}: {err}");
        }
    }

    #[test]
    fn save_and_load_roundtrip() {
        let d = dirs();
        let cfg = sample();
        cfg.save(&d).expect("保存失败");
        assert!(AgentModelConfig::path(&d).is_file());

        let loaded = AgentModelConfig::load(&d).expect("应能读回");
        assert_eq!(loaded, cfg, "roundtrip 内容应一致");
        // 敏感字段确实落了盘（本机数据目录内）
        let raw = std::fs::read_to_string(AgentModelConfig::path(&d)).unwrap();
        assert!(raw.contains("deepseek-chat"));
    }

    #[test]
    fn load_missing_returns_none() {
        let d = AgentDirs::from_root(
            std::env::temp_dir()
                .join(format!("dh_cfg_missing_{}", std::process::id()))
                .join("agents")
                .join("deepharness"),
            "deepharness",
        );
        assert!(AgentModelConfig::load(&d).is_none());
    }

    #[test]
    fn load_corrupt_returns_none_not_panic() {
        let d = dirs();
        std::fs::create_dir_all(&d.config).unwrap();
        std::fs::write(AgentModelConfig::path(&d), "{ not json").unwrap();
        assert!(AgentModelConfig::load(&d).is_none());
    }

    #[test]
    fn save_rejects_invalid_config() {
        let mut cfg = sample();
        cfg.api_key = String::new();
        let d = dirs();
        assert!(cfg.save(&d).is_err());
        assert!(!AgentModelConfig::path(&d).exists(), "校验失败不应写文件");
    }
}
