//! 在系统默认浏览器中打开链接。
//!
//! 为什么不在前端用 `window.open`：
//! 1. 打开地址要等后端解析出一次性 token（实测 dsh 约 5 秒后才打印），
//!    而 `window.open` 必须在用户手势的同步栈里调用才不会被拦截 ——
//!    「await 之后再 open」在 WebView 里既可能被拦，也可能落到一个
//!    我们拿不到句柄的新窗口上，无法再改地址；
//! 2. token 是本机的临时凭据，留在 Rust 侧不流向前端更干净。
//!
//! 安全约束：本模块只接受**回环地址**且字符集受限的 URL。
//! Windows 上通过 `cmd /C start` 打开时，cmd 会重新解析整条命令行，
//! 因此地址里绝不允许出现 `&`、`|`、`^`、引号等元字符 —— 否则即便
//! URL 是我们自己拼出来的，也等于把 shell 注入面留在了调用点上。

use crate::error::{AppError, AppResult};

/// 允许打开的主机前缀（只允许本机回环）。
const ALLOWED_HOST_PREFIXES: [&str; 2] = ["http://127.0.0.1:", "http://localhost:"];

/// URL 中允许出现的字符。
///
/// 覆盖本项目的实际用法：路径、查询串、URL-safe base64 的 token。
/// 刻意排除 shell 元字符（`& | ^ > < " ' \`` 空格）与反斜杠。
fn is_allowed_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            ':' | '/' | '?' | '=' | '-' | '_' | '.' | '~' | '%' | '#' | '@' | '+'
        )
}

/// 该 URL 是否可以被安全地交给系统浏览器打开。
pub fn is_safe_external_url(url: &str) -> bool {
    // 注意 `*prefix`：数组 `.iter()` 产出的是 `&&str`，而 `strip_prefix` 的
    // 模式参数接受的是 `&str`（`&&str` 要靠 std 的一个特例实现兜住，
    // 显式解引用更稳）。
    let Some(rest) = ALLOWED_HOST_PREFIXES
        .iter()
        .find_map(|prefix| url.strip_prefix(*prefix))
    else {
        return false;
    };
    // 前缀之后必须还有端口号，避免 `http://127.0.0.1:` 这种半截地址
    let port_digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if port_digits == 0 {
        return false;
    }
    url.chars().all(is_allowed_char)
}

/// 在系统默认浏览器中打开一个回环地址。
pub fn open_in_system_browser(url: &str) -> AppResult<()> {
    if !is_safe_external_url(url) {
        return Err(AppError::Other(format!(
            "拒绝打开非回环或含非法字符的地址：{}",
            mask_token(url)
        )));
    }

    #[cfg(windows)]
    let spawned = std::process::Command::new("cmd")
        // 空字符串是 `start` 的窗口标题占位，不可省略，
        // 否则带引号的 URL 会被当作标题而根本不打开。
        .args(["/C", "start", "", url])
        .spawn();

    // 非 Windows 平台的等价实现（本项目主要面向 Windows，
    // 这里保留可编译的分支，避免 cfg 之外的代码路径缺失）。
    #[cfg(target_os = "macos")]
    let spawned = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let spawned = std::process::Command::new("xdg-open").arg(url).spawn();

    spawned.map_err(|e| AppError::Other(format!("调用系统浏览器失败: {e}")))?;
    Ok(())
}

/// 把 token 换成 `***`，用于日志与错误信息。
///
/// 地址本身来自本机日志，但 token 是一次性的会话凭据，
/// 不该出现在任何会被展示或落盘的文本里。
pub fn mask_token(url: &str) -> String {
    match url.find("token=") {
        Some(idx) => format!("{}token=***", &url[..idx]),
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_loopback_with_token() {
        assert!(is_safe_external_url(
            "http://127.0.0.1:3080/?token=BXXfmmi_ZCnraBcWXgUfQLjebJLUzyQzh8lZEI36vbU"
        ));
        assert!(is_safe_external_url("http://localhost:3080/"));
    }

    #[test]
    fn rejects_shell_metacharacters() {
        // 这些字符会被 cmd 重新解析，必须拒绝
        for bad in [
            "http://127.0.0.1:3080/?a=1&b=2",
            "http://127.0.0.1:3080/|whoami",
            "http://127.0.0.1:3080/^&calc",
            "http://127.0.0.1:3080/?x=\"y\"",
            "http://127.0.0.1:3080/?x='y'",
            "http://127.0.0.1:3080/?x=`y`",
            "http://127.0.0.1:3080/?x=a b",
            "http://127.0.0.1:3080/\\..\\..",
        ] {
            assert!(!is_safe_external_url(bad), "应当拒绝：{bad}");
        }
    }

    #[test]
    fn rejects_non_loopback_and_partial_urls() {
        for bad in [
            "http://example.com/",
            "https://127.0.0.1:3080/",
            "http://127.0.0.1:",
            "file:///C:/Windows/System32/calc.exe",
            "",
            "javascript:alert(1)",
        ] {
            assert!(!is_safe_external_url(bad), "应当拒绝：{bad}");
        }
    }

    #[test]
    fn open_rejects_unsafe_url() {
        assert!(open_in_system_browser("http://example.com/").is_err());
    }

    #[test]
    fn mask_hides_token() {
        assert_eq!(
            mask_token("http://127.0.0.1:3080/?token=SECRETSECRETSECRETSECRET"),
            "http://127.0.0.1:3080/?token=***"
        );
        assert_eq!(mask_token("http://127.0.0.1:3080/"), "http://127.0.0.1:3080/");
    }
}
