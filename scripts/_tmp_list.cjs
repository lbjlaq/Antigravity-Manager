const { execSync } = require('child_process');
const out = execSync('curl.exe -sS --connect-timeout 20 "https://mirrors.tuna.tsinghua.edu.cn/rustup/dist/2026-09-27/"', { encoding: 'utf8', maxBuffer: 20 * 1024 * 1024 });
const links = out.match(/href="([^"]+)"/g) || [];
for (const l of links) {
  if (l.includes('x86_64-pc-windows-msvc') && l.includes('tar.xz') && !l.includes('.sha256')) console.log(l);
}
