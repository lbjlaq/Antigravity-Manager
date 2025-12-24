/**
 * 模拟 Tauri Dialog 插件
 */

export async function open(options: any = {}): Promise<string | null> {
    console.log('[Mock Dialog] Open:', options);
    // Web 无法选择服务器路径，返回空或提示
    alert('Web 版本不支持直接选择本地路径。');
    return null;
}

export async function save(options: any = {}): Promise<string | null> {
    console.log('[Mock Dialog] Save:', options);
    const filename = options.defaultPath || 'export.json';
    // 在 Web 中，我们通常在这里不返回路径，而是直接触发下载
    // 但为了兼容代码，这里返回一个虚拟路径
    return filename;
}
