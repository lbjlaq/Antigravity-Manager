import { invoke } from '@tauri-apps/api/core';
import { AppConfig } from '../types/config';

export async function loadConfig(): Promise<AppConfig> {
    return await invoke('load_config');
}

export async function saveConfig(config: AppConfig): Promise<void> {
    return await invoke('save_config', { config });
}

export async function getAntigravityPath(): Promise<string> {
    return await invoke('get_antigravity_path');
}

export async function setAntigravityPath(path: string | null): Promise<void> {
    return await invoke('set_antigravity_path', { path });
}

export async function detectAntigravityPath(): Promise<string | null> {
    return await invoke('detect_antigravity_path');
}
