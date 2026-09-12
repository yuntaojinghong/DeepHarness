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

/// 允许打开的回环主机前缀（dsh 等本机服务）。
const ALLOWED_HOST_PREFIXES: [&str; 2] = ["http://127.0.0.1:", "http://localhost:"];

/// 允许打开的公网站点白名单（仅 https）。
///
/// 采用白名单而不是「放行任意 https」：该命令可被前端任意调用，
/// 白名单把「渲染层万一被注入」时的利用面压到最小。新增站点需在此登记。
const ALLOWED_WEB_HOSTS: [&str; 2] = ["github.com", "platform.deepseek.com"];

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

/// 是否为允许打开的本机回环地址。
fn is_loopback_url(url: &str) -> bool {
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
    rest.chars().take_while(|c| c.is_ascii_digit()).count() > 0
}

/// 是否为白名单内的公网 https 地址。
fn is_allowed_web_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    // host 截止到路径 / 查询 / 片段的第一个分隔符，再去掉可能的端口
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    if host.is_empty() {
        return false;
    }
    ALLOWED_WEB_HOSTS
        .iter()
        .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")))
}

/// 该 URL 是否可以被安全地交给系统浏览器打开。
pub fn is_safe_external_url(url: &str) -> bool {
    // 字符白名单对两类地址都适用；先过这一关，后面就不必再担心 shell 元字符。
    if !url.chars().all(is_allowed_char) {
        return false;
    }
    is_loopback_url(url) || is_allowed_web_url(url)
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
    let spawned = crate::process::command("cmd")
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

/// 在系统默认浏览器中打开一个外部链接（回环地址或白名单内的公网站点）。
///
/// 抽成独立命令的理由与官方 Web UI 相同：`<a target="_blank">` 与
/// `window.open` 在 Tauri WebView 里的行为并不等价于浏览器 —— 前者可能
/// 在 WebView 内部打开、把应用界面替换掉。统一走后端 `start` 才能保证
/// 「一定落到用户的默认浏览器」，并且顺带获得 URL 校验。
#[tauri::command]
pub fn open_external_url(url: String) -> AppResult<()> {
    open_in_system_browser(&url)
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
    fn accepts_whitelisted_web_urls() {
        assert!(is_safe_external_url(
            "https://github.com/yuntaojinghong/DeepHarness"
        ));
        assert!(is_safe_external_url(
            "https://github.com/yuntaojinghong/DeepHarness/releases"
        ));
        assert!(is_safe_external_url("https://github.com/"));
        // 白名单站点下的子域一并放行
        assert!(is_safe_external_url("https://gist.github.com/abc"));
        assert!(is_safe_external_url("https://platform.deepseek.com/usage"));
    }

    #[test]
    fn rejects_web_urls_outside_the_whitelist() {
        for bad in [
            "https://example.com/",
            "http://github.com/",          // 公网站点必须 https
            "https://evilgithub.com/",     // 后缀必须落在 `.github.com` 上
            "https://github.com.evil.com/", // 不能用白名单域名做前缀
            "https://",
        ] {
            assert!(!is_safe_external_url(bad), "应当拒绝：{bad}");
        }
    }

    #[test]
    fn rejects_shell_metacharacters_in_web_urls_too() {
        // 字符白名单对公网地址同样生效（`&` 会被 cmd 重新解析）
        assert!(!is_safe_external_url("https://github.com/a?b=1&c=2"));
        assert!(!is_safe_external_url("https://github.com/a|whoami"));
    }

    #[test]
    fn open_external_url_rejects_unsafe_targets() {
        assert!(open_external_url("http://example.com/".to_string()).is_err());
        assert!(open_external_url("https://evil.com/".to_string()).is_err());
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
