use sysinfo::System;

fn main() {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);

    let mut immune_exe_paths = std::collections::HashSet::new();
    for (_pid, process) in system.processes() {
        let args = process.cmd();
        let args_str = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_lowercase())
            .collect::<Vec<String>>()
            .join(" ");
        
        if args_str.contains("antigravity ide") || args_str.contains("antigravity-ide") {
            if let Some(exe_path) = process.exe().and_then(|p| p.to_str()) {
                immune_exe_paths.insert(exe_path.to_lowercase());
            }
        }
    }
    
    println!("Immune paths: {:?}", immune_exe_paths);
    
    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy().to_lowercase();
        let exe_path = process.exe().and_then(|p| p.to_str()).unwrap_or("").to_lowercase();
        if !name.contains("antigravity") { continue; }
        
        let is_ide_match = if immune_exe_paths.contains(&exe_path) {
            false 
        } else {
            (exe_path.contains("antigravity") || name.contains("antigravity"))
                && !exe_path.contains("antigravity ide")
                && !exe_path.contains("antigravity-ide")
                && !name.contains("antigravity ide")
                && !name.contains("antigravity-ide")
        };
        
        let is_helper = process.cmd().iter().any(|arg| arg.to_string_lossy().to_lowercase().contains("--type="));
        
        if is_ide_match && !is_helper {
            println!("PID {} ({}) WILL BE KILLED", pid, exe_path);
        }
    }
}
