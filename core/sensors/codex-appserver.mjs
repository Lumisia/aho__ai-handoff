import { spawn } from 'node:child_process';
import { INIT_REQUEST, reduce } from './appserver-protocol.mjs';

export function appServerSpawnSpec(command = 'codex', {
  platform = process.platform, comspec = process.env.ComSpec || process.env.COMSPEC || 'cmd.exe',
} = {}) {
  if (!/^[A-Za-z0-9_./:\\ -]+$/.test(command)) throw new Error('unsafe command path');
  if (platform === 'win32') {
    return {
      file: comspec,
      args: ['/d', '/s', '/c', `"${command}" app-server --stdio`],
      options: { shell: false, windowsHide: true, windowsVerbatimArguments: true },
    };
  }
  return {
    file: command, args: ['app-server', '--stdio'], options: { shell: false, windowsHide: true },
  };
}

// codex app-server --stdio 를 spawn 해 handshake 후 5h 한도를 읽는다.
// 실패·타임아웃이면 null. (milestone 1 spike에서 실측 검증된 흐름.)
export async function readAppServerRateLimit({ timeoutMs = 15000, command = 'codex', onStderr } = {}) {
  return new Promise((resolve) => {
    let child;
    try {
      const spec = appServerSpawnSpec(command);
      child = spawn(spec.file, spec.args, spec.options);
    } catch {
      resolve(null);
      return;
    }

    let buf = '';
    let settled = false;
    const finish = (val) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try { child.kill(); } catch {}
      resolve(val);
    };
    const timer = setTimeout(() => finish(null), timeoutMs);
    const send = (o) => { try { child.stdin.write(JSON.stringify(o) + '\n'); } catch {} };

    child.stdout.on('data', (d) => {
      buf += d.toString();
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i).trim();
        buf = buf.slice(i + 1);
        if (!line) continue;
        let msg;
        try { msg = JSON.parse(line); } catch { continue; }
        const out = reduce(msg);
        if (out.send) out.send.forEach(send);
        if (out.result !== undefined) finish(out.result);
        if (out.error !== undefined) finish(null);
      }
    });
    child.stderr.on('data', (data) => onStderr?.(data.toString()));
    child.on('error', () => finish(null));
    child.on('exit', () => finish(null));

    send(INIT_REQUEST);
  });
}
