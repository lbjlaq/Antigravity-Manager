/**
 * Mock implementation of @tauri-apps/api/path for Web
 */

export async function join(...paths: string[]): Promise<string> {
    return paths.join('/');
}
