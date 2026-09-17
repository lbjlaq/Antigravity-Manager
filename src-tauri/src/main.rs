fn main() {
    // 强制默认以 Headless 独立 Web 服务端模式运行
    let mut args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|arg| arg == "--headless") {
        args.push("--headless".to_string());
    }

    antigravity_tools_lib::run()
}
