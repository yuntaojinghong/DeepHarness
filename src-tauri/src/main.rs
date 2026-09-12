#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Sidecar 模式：`DeepHarness --agent-worker <agent_id>` 以 JSON Lines
    // 协议在独立进程中为自研 Agent 服务（见 agents::worker 模块）。
    // Worker 模式下不启动任何 GUI。
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "--agent-worker" {
        let mut stdin = std::io::BufReader::new(std::io::stdin().lock());
        let mut stdout = std::io::stdout().lock();
        let _exit = deepharness_lib::agents::worker::run_worker_loop(&mut stdin, &mut stdout);
        return;
    }
    deepharness_lib::run()
}
