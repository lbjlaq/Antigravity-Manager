use std::fs;
use std::path::Path;
use std::process::Command;

#[tauri::command]
pub async fn patch_agy_binary(file_path: String) -> Result<String, String> {
    let mut actual_path = file_path.clone();
    if actual_path.ends_with(".app") || actual_path.ends_with(".app/") {
        let app_path = Path::new(&actual_path);
        let inner = app_path.join("Contents/MacOS/agy");
        if inner.exists() {
            actual_path = inner.to_string_lossy().to_string();
        }
    }

    let path = Path::new(&actual_path);
    if !path.exists() {
        return Err("File not found".into());
    }

    let data = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let n = data.len();
    let mut patch_offset = None;
    let mut new_inst_bytes = None;
    let mut is_pe_x64 = false;

    // 1. Scan for x86_64 PE (Windows/Linux) pattern
    // Pattern: cmpb $0x0, (%r12) -> 41 80 3c 24 00
    //          jne offset32      -> 0f 85 XX XX XX XX
    //          leaq rip_off, rax -> 48 8d 05 XX XX XX XX
    //          mov $0x18, %ebx   -> bb 18 00 00 00
    let pe_pattern = [0x41, 0x80, 0x3c, 0x24, 0x00, 0x0f, 0x85];
    let mut i = 0;
    while i < n - 25 {
        if data[i..i+7] == pe_pattern {
            // Validate the rest of the pattern
            // leaq opcode starts after jne (which is 6 bytes: 0f 85 XX XX XX XX)
            let leaq_idx = i + 5 + 6;
            if data[leaq_idx..leaq_idx+3] == [0x48, 0x8d, 0x05] {
                // mov $0x18, %ebx starts after leaq (which is 7 bytes: 48 8d 05 XX XX XX XX)
                let mov_idx = leaq_idx + 7;
                if data[mov_idx..mov_idx+2] == [0xbb, 0x18] {
                    // Found the gate!
                    patch_offset = Some(i + 5); // Points to the jne instruction: 0f 85 ...
                    // Rewrite jne to 6 NOP bytes (0x90) so it falls through unconditionally
                    new_inst_bytes = Some(vec![0x90; 6]);
                    is_pe_x64 = true;
                    break;
                }
            }
        }
        i += 1;
    }

    // 2. Scan for ARM64 eligibility gate pattern if not PE x86_64
    if patch_offset.is_none() {
        for j in (0..n - 20).step_by(4) {
            let inst1 = u32::from_le_bytes(data[j..j+4].try_into().unwrap());
            let inst2 = u32::from_le_bytes(data[j+4..j+8].try_into().unwrap());
            let inst4 = u32::from_le_bytes(data[j+12..j+16].try_into().unwrap());
            let inst5 = u32::from_le_bytes(data[j+16..j+20].try_into().unwrap());

            // 1. ldrb wA, [xB, #0x58]
            if (inst1 & 0xfffffc00) != 0x39416000 {
                continue;
            }
            let b_reg = (inst1 >> 5) & 0x1f;
            let a_reg = inst1 & 0x1f;

            // 2. tbnz wA, #0, label1
            if (inst2 & 0xffe0001f) != (0x37000000 | a_reg) {
                continue;
            }

            // 3. ldr xC, [xB, #0x38]
            if (inst4 & 0xfffffc00) != 0xf9401c00 || ((inst4 >> 5) & 0x1f) != b_reg {
                continue;
            }
            let c_reg = inst4 & 0x1f;

            // 4. cbz xC, label_send
            if (inst5 & 0xffe0001f) != (0xb4000000 | c_reg) {
                continue;
            }

            // Extract imm19 from cbz
            let imm19_raw = (inst5 >> 5) & 0x7ffff;
            let imm19 = if (imm19_raw & 0x40000) != 0 {
                (imm19_raw as i32) - 0x80000
            } else {
                imm19_raw as i32
            };

            patch_offset = Some(j + 16);
            // Encode unconditional branch: b label_send (0x14000000 | (imm19 & 0x3ffffff))
            let b_inst = 0x14000000 | ((imm19 as u32) & 0x3ffffff);
            new_inst_bytes = Some(b_inst.to_le_bytes().to_vec());
            break;
        }
    }

    if patch_offset.is_none() {
        // Check if already patched for x86_64 PE
        let mut check_idx = 0;
        while check_idx < n - 25 {
            if data[check_idx..check_idx+7] == pe_pattern {
                let leaq_idx = check_idx + 5 + 6;
                if data[leaq_idx..leaq_idx+3] == [0x48, 0x8d, 0x05] {
                    let mov_idx = leaq_idx + 7;
                    if data[mov_idx..mov_idx+2] == [0xbb, 0x18] {
                        if data[check_idx+5..check_idx+11] == [0x90; 6] {
                            return Ok("Binary is already patched.".into());
                        }
                    }
                }
            }
            check_idx += 1;
        }

        // Check if already patched for ARM64
        for j in (0..n - 20).step_by(4) {
            let inst1 = u32::from_le_bytes(data[j..j+4].try_into().unwrap());
            let inst2 = u32::from_le_bytes(data[j+4..j+8].try_into().unwrap());
            let inst4 = u32::from_le_bytes(data[j+12..j+16].try_into().unwrap());
            let inst5 = u32::from_le_bytes(data[j+16..j+20].try_into().unwrap());

            if (inst1 & 0xfffffc00) == 0x39416000 {
                let b_reg = (inst1 >> 5) & 0x1f;
                let a_reg = inst1 & 0x1f;
                if (inst2 & 0xffe0001f) == (0x37000000 | a_reg) {
                    if (inst4 & 0xfffffc00) == 0xf9401c00 && ((inst4 >> 5) & 0x1f) == b_reg {
                        if (inst5 & 0xfc000000) == 0x14000000 {
                            return Ok("Binary is already patched.".into());
                        }
                    }
                }
            }
        }

        return Err("Pattern not found. This version of the CLI might not have the eligibility gate, or the structure has changed.".into());
    }

    let offset = patch_offset.unwrap();
    let patch_bytes = new_inst_bytes.unwrap();

    // Create backup
    let backup_path = format!("{}.bak", actual_path);
    if !Path::new(&backup_path).exists() {
        fs::copy(path, &backup_path).map_err(|e| format!("Failed to create backup: {}", e))?;
    }

    // Apply patch
    use std::io::{Seek, SeekFrom, Write};
    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|e| format!("Failed to open file for writing: {}", e))?;
    file.seek(SeekFrom::Start(offset as u64))
        .map_err(|e| format!("Seek failed: {}", e))?;
    file.write_all(&patch_bytes)
        .map_err(|e| format!("Write failed: {}", e))?;

    // Re-sign on macOS (only if we patched an ARM64 macOS executable)
    #[cfg(target_os = "macos")]
    {
        if !is_pe_x64 {
            let _ = Command::new("codesign")
                .args(&["--remove-signature", &actual_path])
                .output();
            let output = Command::new("codesign")
                .args(&["--sign", "-", &actual_path])
                .output();
            match output {
                Ok(out) if out.status.success() => {},
                Ok(out) => {
                    let err_msg = String::from_utf8_lossy(&out.stderr);
                    return Err(format!("Patch applied, but codesigning failed: {}", err_msg));
                },
                Err(e) => return Err(format!("Patch applied, but codesigning execution failed: {}", e)),
            }
        }
    }

    Ok("Patch applied successfully!".into())
}
