// Materialize a literal command for the existing Windows lab command workflow.
// No script text is read/evaluated in the guest and no execution policy is changed.
import {readFileSync} from 'node:fs';
const text=readFileSync(new URL('./windows-rust-bridge-check.ps1',import.meta.url),'utf8');
const lines=text.split('\n').map(s=>s.trim()).filter(s=>s&&!s.startsWith('#'));
const command=lines.map(s=>s+(/[({,]$/.test(s)?'':';')).join(' ').replace(/"([^"]*)"/g,(_,s)=>
  '('+s.split(/(\$[a-zA-Z]+)/).filter(Boolean).map(v=>v.startsWith('$')?v:"'"+v+"'").join('+')+')');
if(command.length>7900) throw Error('CMD command length exceeded');
process.stdout.write('@echo off\nrem Explicit command invocation; no policy or trust-store changes.\npowershell.exe -NoProfile -NonInteractive -Command "'+command+'" > C:\\MixRelayLab\\rust-bridge-check-launch.log 2>&1\nexit /b %errorlevel%\n');
