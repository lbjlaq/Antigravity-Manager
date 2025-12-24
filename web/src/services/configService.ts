import { invoke } from '../utils/invoke';
import { AppConfig } from '../types/config';

export async function loadConfig(): Promise<AppConfig> {
    return await invoke('load_config');
}

export async function saveConfig(config: AppConfig): Promise<void> {
    return await invoke('save_config', { config });
}
