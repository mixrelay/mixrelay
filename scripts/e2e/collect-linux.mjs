import { spawnSync } from 'node:child_process';
import { readFileSync, readdirSync, lstatSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

export function collectLinux(output) {
  function capture(file, command, args, timeout = 15000) {
    const result = spawnSync(command, args, { encoding: 'utf8', timeout, maxBuffer: 8 * 1024 * 1024 });
    writeFileSync(join(output, file), `${result.stdout ?? ''}\n${result.stderr ?? ''}\nexit=${result.status}; error=${result.error?.message ?? ''}\n`, { flag: 'wx' });
  }
  capture('driver-journal.txt', 'journalctl', ['--user', '-u', 'mixrelay-driver.service', '--no-pager', '-n', '500']);
  capture('pipewire-state.json', 'pw-dump', []);
  for (const pid of readdirSync('/proc').filter(id => /^\d+$/.test(id))) {
    try {
      if (lstatSync(`/proc/${pid}`).uid !== process.getuid()) continue;
      const executable = readFileSync(`/proc/${pid}/cmdline`).toString().split('\0')[0];
      if (!['/usr/bin/mixrelay-engine-service', '/usr/bin/mixrelay-driver-service'].includes(executable)) continue;
      capture(`${executable.split('/').at(-1)}-${pid}.stack.txt`, 'sudo', ['-n', 'gdb', '--batch', '--nx', '-p', pid,
        '-ex', 'set pagination off', '-ex', 'thread apply all bt', '-ex', 'detach']);
    } catch (error) { if (!['ENOENT', 'ESRCH'].includes(error.code)) throw error; }
  }
}
