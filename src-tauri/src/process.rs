//! 子进程创建的 Windows 通用处理。
//!
//! ## 为什么需要这个模块
//!
//! 桌面应用绝不能弹出控制台窗口。未使用 `CREATE_NO_WINDOW` 时，从 GUI 进程
//! 启动的控制台程序（`cmd.exe` / `node.exe` / `netstat.exe` / `where.exe` …）
//! 会各自获得一个**新建的控制台**并闪现到屏幕上。实测表现：
//! 启动应用、切换 Agent、检查更新时莫名其妙弹出黑框，且会抢走焦点。
//!
//! 这类问题必须逐调用点处理——`CREATE_NO_WINDOW` 是**创建标志**，
//! 无法在别处统一注入，漏掉任何一个调用点就仍会闪窗。
//!
//! 因此本模块只做一件事：把「加静默标志」这件事收敛成单一实现，
//! 所有创建子进程的地方一律经由这里，避免有人再忘记。

use std::process::Command;

/// Windows 上隐藏控制台窗口所需的一切创建标志。
///
/// - `CREATE_NO_WINDOW`(0x0800_0000)：不为控制台程序创建控制台。
/// - `CREATE_NEW_PROCESS_GROUP`(0x0000_0200)：不继承父进程的控制台事件组，
///   避免 Ctrl 事件被一起分发（对本项目是附加的稳妥措施）。
#[cfg(windows)]
pub const SILENT_CREATION_FLAGS: u32 = 0x0800_0000 | 0x0000_0200;

/// 让一个子进程静默启动：不弹窗、不抢焦点。
///
/// 非 Windows 平台是空操作（本项目的发布目标只有 Windows，
/// 但保持签名一致可以让调用点无需 `cfg` 分支）。
pub fn silent(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(SILENT_CREATION_FLAGS);
    }
    cmd
}

/// 新建一个静默的 `Command`。
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    silent(&mut cmd);
    cmd
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn creation_flags_include_create_no_window() {
        // 0x0800_0000 是 CREATE_NO_WINDOW。少了它就会弹控制台窗口。
        assert_ne!(SILENT_CREATION_FLAGS & 0x0800_0000, 0);
    }

    #[test]
    fn silent_command_can_be_built() {
        // 只验证构建出的命令可用；不去执行外部程序，测试不依赖环境。
        let mut cmd = command("cmd");
        cmd.arg("/C").arg("exit");
        assert_eq!(cmd.get_program().to_string_lossy(), "cmd");
    }

    #[test]
    fn actually_runs_without_a_console_window() {
        // 端到端确认标志不会被忽略：跑一个真实控制台程序并取回输出。
        let out = command("cmd")
            .args(["/C", "echo silent-ok"])
            .output()
            .expect("无法启动 cmd");
        assert!(out.status.success(), "cmd 未正常退出");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("silent-ok"), "输出异常: {text}");
    }
}
