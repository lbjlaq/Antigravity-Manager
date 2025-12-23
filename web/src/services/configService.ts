/**
 * 配置服务 - Web 版本
 * 使用 localStorage 和 HTTP API
 */

import { api } from './api';
import { AppConfig } from '../types/config';

// 本地存储键
const CONFIG_KEY = 'antigravity_config';

// 默认配置
const DEFAULT_CONFIG: AppConfig = {
    theme: 'dark',
    language: 'zh-CN',
    proxy: {
        port: 8045,
        autoStart: false,
        apiKey: 'sk-antigravity',
        modelMapping: {},
        requestTimeout: 120,
    },
};

export async function loadConfig(): Promise<AppConfig> {
    // 优先从 localStorage 加载本地配置
    const localConfig = localStorage.getItem(CONFIG_KEY);
    let config = localConfig ? JSON.parse(localConfig) : { ...DEFAULT_CONFIG };

    // 尝试从服务器获取配置（仅在线时）
    try {
        const serverConfig = await api.config.get();
        // 合并服务器配置（服务端配置优先用于 proxy 部分）
        config = {
            ...config,
            proxy: {
                ...config.proxy,
                ...serverConfig.proxy,
                port: serverConfig.server?.port || config.proxy.port,
            },
        };
    } catch (error) {
        console.warn('无法从服务器获取配置，使用本地配置:', error);
    }

    return config;
}

export async function saveConfig(config: AppConfig): Promise<void> {
    // 保存到 localStorage
    localStorage.setItem(CONFIG_KEY, JSON.stringify(config));

    // 尝试同步到服务器（仅代理相关配置）
    try {
        await api.config.update({
            request_timeout: config.proxy.requestTimeout,
            model_mapping: config.proxy.modelMapping,
        });
    } catch (error) {
        console.warn('无法同步配置到服务器:', error);
    }
}
