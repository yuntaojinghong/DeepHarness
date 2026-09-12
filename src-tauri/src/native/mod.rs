//! DeepHarness Native Agent（自研 Agent）核心。
//!
//! 模块职责：
//! - [`model`]：模型提供方抽象（`ModelProvider` trait）+ DeepSeek 实现
//!   （Windows WinHTTP 原生 TLS，零新增编译风险）；
//! - [`memory`]：SQLite 长期记忆库（rusqlite bundled，无需系统 SQLite）；
//! - [`tools`]：工具注册表与执行（全部工具经由权限白名单校验）；
//! - [`planner`]：多步规划器（模型输出结构化 JSON 计划）；
//! - [`reflector`]：反思与纠错（对每步执行结果进行结构化评审）；
//! - [`executor`]：编排规划 → 执行 → 反思的完整任务循环。
//!
//! 隔离边界：本模块全部代码都运行在 Worker 侧car 进程中
//! （`current_exe --agent-worker`，见 `agents::worker`），任何崩溃只影响
//! 自研 Agent 自身；所有文件操作必须经过 `permissions` 白名单。

pub mod executor;
pub mod memory;
pub mod model;
pub mod planner;
pub mod reflector;
pub mod tools;
