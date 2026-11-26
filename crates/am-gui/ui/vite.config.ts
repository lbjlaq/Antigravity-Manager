import { defineConfig, Plugin } from 'vite';
import react from '@vitejs/plugin-react';
import fs from 'fs';
import path from 'path';
import os from 'os';

function antigravityDevApiPlugin(): Plugin {
  return {
    name: 'antigravity-dev-api',
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        const url = req.url?.split('?')[0];

        if (url === '/api/accounts' && req.method === 'GET') {
          try {
            const home = os.homedir();
            const accountsDir = path.join(home, '.antigravity_tools', 'accounts');
            const accounts: any[] = [];
            const seenIds = new Set<string>();

            // 1. 尝试读取 accounts.json 索引
            const indexPath = path.join(home, '.antigravity_tools', 'accounts.json');
            if (fs.existsSync(indexPath)) {
              try {
                const indexData = JSON.parse(fs.readFileSync(indexPath, 'utf-8'));
                if (Array.isArray(indexData.accounts)) {
                  for (const s of indexData.accounts) {
                    const accFile = path.join(accountsDir, `${s.id}.json`);
                    if (fs.existsSync(accFile)) {
                      try {
                        const acc = JSON.parse(fs.readFileSync(accFile, 'utf-8'));
                        seenIds.add(acc.id);
                        accounts.push(acc);
                      } catch {}
                    }
                  }
                }
              } catch {}
            }

            // 2. 补全 accounts 目录未在索引中的其他账号文件
            if (fs.existsSync(accountsDir)) {
              const files = fs.readdirSync(accountsDir).filter((f) => f.endsWith('.json'));
              for (const file of files) {
                const id = path.basename(file, '.json');
                if (!seenIds.has(id)) {
                  try {
                    const acc = JSON.parse(fs.readFileSync(path.join(accountsDir, file), 'utf-8'));
                    seenIds.add(acc.id);
                    accounts.push(acc);
                  } catch {}
                }
              }
            }

            res.setHeader('Content-Type', 'application/json');
            res.end(JSON.stringify(accounts));
            return;
          } catch (e: any) {
            res.statusCode = 500;
            res.end(JSON.stringify({ error: e.message }));
            return;
          }
        }

        if (url === '/api/stats' && req.method === 'GET') {
          try {
            const { execSync } = require('child_process');
            const binPath = path.resolve(__dirname, '../../../target/debug/am');
            const out = execSync(`"${binPath}" stats --json`, { encoding: 'utf-8' });
            res.setHeader('Content-Type', 'application/json');
            res.end(out);
            return;
          } catch (e: any) {
            res.statusCode = 500;
            res.end(JSON.stringify({ error: e.message }));
            return;
          }
        }

        if (url === '/api/hosts' && req.method === 'GET') {
          try {
            const home = os.homedir();
            let appAcc: string | null = null;
            let ideAcc: string | null = null;

            const activeJson = path.join(home, '.gemini', 'antigravity', 'active_account.json');
            if (fs.existsSync(activeJson)) {
              try {
                const val = JSON.parse(fs.readFileSync(activeJson, 'utf-8'));
                appAcc = val.email || null;
                ideAcc = val.email || null;
              } catch {}
            }

            const hosts = {
              app_account: appAcc,
              app_running: false,
              ide_account: ideAcc,
              ide_running: false,
              plugin_account: null,
              plugin_running: false,
              agy_account: appAcc,
            };

            res.setHeader('Content-Type', 'application/json');
            res.end(JSON.stringify(hosts));
            return;
          } catch (e: any) {
            res.statusCode = 500;
            res.end(JSON.stringify({ error: e.message }));
            return;
          }
        }

        next();
      });
    },
  };
}

export default defineConfig({
  plugins: [react(), antigravityDevApiPlugin()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    target: process.env.TAURI_PLATFORM == 'windows' ? 'chrome105' : 'safari13',
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
});
