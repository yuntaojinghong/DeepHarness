//! 日志初始化：stdout + 滚动日志文件双输出。
//!
//! 主应用日志写入 `<base>/logs/app.log.YYYY-MM-DD`；
//! 每个 Agent 后续拥有自己 logs/ 目录下的独立日志（Agent 隔离层接入时使用）。

use std::path::Path;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// 初始化全局 tracing 订阅者（进程内只调用一次）。
///
/// - 日志级别由环境变量 `DEEPHARNESS_LOG` / `RUST_LOG` 控制，默认 `info`；
/// - 同时输出到 stdout 与按天滚动的文件（保留在 logs 目录下）；
/// - 重复调用是空操作，避免测试或多窗口场景重复注册。
pub fn init(base_logs_dir: &Path) {
    if tracing::dispatcher::has_been_set() {
        return;
    }

    let file_appender = tracing_appender::rolling::daily(base_logs_dir, "app.log");
    let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);
    // 泄漏 guard 以保证日志写入器存活整个进程生命周期；
    // 只发生一次，量级可忽略，且比全局 Option + shutdown hook 更简单可靠。
    std::mem::forget(_guard);

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let result = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(true))
        .with(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(file_writer))
        .try_init();

    if let Err(e) = result {
        // 订阅器已被设置时的竞态是良性的；其余情况打印到 stderr 便于排查
        eprintln!("tracing 初始化失败: {e}");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn init_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        super::init(tmp.path());
        // 第二次调用必须不 panic（订阅器已设置时静默跳过）
        super::init(tmp.path());
    }
}
