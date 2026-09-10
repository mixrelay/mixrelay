#!/usr/bin/env node
// JSON is canonical. Rendering is deterministic so status and release gates cannot drift
// between a hand-edited table and the machine-readable management record.
import { readFileSync, existsSync, writeFileSync } from 'node:fs';
import { dirname, resolve, relative, isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const input = resolve(root, 'docs/feature-parity-matrix.json');
const output = resolve(root, 'docs/feature-parity-matrix.md');
const args = new Set(process.argv.slice(2));
for (const arg of args) {
  if (!['--check', '--release', '--self-test'].includes(arg)) throw new Error('Unknown argument: ' + arg);
}
const statuses = ['missing', 'partial', 'implemented', 'in_progress', 'not_applicable'];
const levels = ['source', 'unit', 'component', 'native_fixture', 'native_device', 'production'];
const platforms = ['macOS', 'Windows', 'Linux'];
const issueStates = ['open', 'in_progress', 'closed', 'scope_review'];

function validate(matrix, checkPaths = true) {
  if (matrix.schemaVersion !== 1 || !Array.isArray(matrix.features)) throw new Error('Invalid matrix schema');
  const ids = new Set();
  for (const feature of matrix.features) {
    if (!/^[a-z][a-z0-9_]+$/.test(feature.id) || ids.has(feature.id)) throw new Error('Invalid/duplicate feature id: ' + feature.id);
    ids.add(feature.id);
    if (!['P0', 'P1', 'P2'].includes(feature.priority)) throw new Error('Invalid priority: ' + feature.id);
    if (!feature.owner || !feature.acceptance || !feature.residual) throw new Error('Missing ownership/acceptance/residual: ' + feature.id);
    if (feature.issue?.id !== 'PARITY-' + feature.id || !issueStates.includes(feature.issue?.state)) throw new Error('Invalid issue: ' + feature.id);
    for (const platform of platforms) {
      const cell = feature.platforms?.[platform];
      if (!cell || !statuses.includes(cell.implementation)) throw new Error('Invalid implementation: ' + feature.id + '/' + platform);
      if (!levels.includes(cell.verification) || !levels.includes(cell.requiredVerification)) throw new Error('Invalid verification: ' + feature.id + '/' + platform);
      if (!Array.isArray(cell.evidence) || !cell.evidence.length) throw new Error('Missing evidence: ' + feature.id + '/' + platform);
      if (cell.implementation === 'not_applicable' && !cell.rationale) throw new Error('Not-applicable needs rationale');
      for (const evidence of cell.evidence) {
        if (!['source', 'test', 'validation'].includes(evidence.kind) || typeof evidence.path !== 'string') throw new Error('Invalid evidence');
        const target = resolve(root, evidence.path);
        const local = relative(root, target);
        if (isAbsolute(evidence.path) || local === '..' || local.startsWith('..' + '/') || local.startsWith('..' + '\\')) throw new Error('Evidence escapes repository');
        if (checkPaths && !existsSync(target)) throw new Error('Missing evidence path: ' + evidence.path);
      }
    }
  }
}
function openGates(matrix) {
  return matrix.features.filter((f) => ['P0', 'P1'].includes(f.priority) && (
    f.issue.state !== 'closed' ||
    platforms.some((os) => {
      const c = f.platforms[os];
      return c.implementation !== 'not_applicable' && (
        c.implementation !== 'implemented' || levels.indexOf(c.verification) < levels.indexOf(c.requiredVerification)
      );
    })
  ));
}
function escaped(value) { return String(value).replaceAll('|', '\\|').replaceAll('\n', ' '); }
function render(matrix) {
  const gates = openGates(matrix);
  const text = [
    '# MixRelay 기능 / OS 동등성 매트릭스', '',
    '> Generated from feature-parity-matrix.json. Edit JSON, then run node scripts/check-feature-parity.mjs.', '',
    '기준: ' + matrix.updatedAt + '. 기능 동등성과 플러그인 형식 동일성은 다릅니다. 단위 테스트·빌드는 native acceptance를 대신하지 않습니다.', '',
    '**열린 출시 게이트: ' + gates.length + '개 (P0 ' + gates.filter((f) => f.priority === 'P0').length + ', P1 ' + gates.filter((f) => f.priority === 'P1').length + '). 전 OS 완료 선언 불가.**', '',
    '## 판정 규칙', '',
    ...matrix.rules.map((rule) => '- ' + rule), '',
    '검증 순서: ' + levels.join(' → ') + '. 이는 증거 종류의 순서이지 기능 완성도 점수가 아닙니다. native_fixture는 실제 API를 사용한 합성 장치/테스트 플러그인; native_device도 물리 64채널 인터페이스나 장시간 방송 합격을 자동 의미하지 않습니다.', '',
    '## 기능 매트릭스', '',
    '| ID / 우선순위 | 기능 | macOS | Windows | Linux | 담당 / 추적 |',
    '|---|---|---|---|---|---|',
    ...matrix.features.map((f) => '| ' + [f.id + ' / ' + f.priority, f.title, ...platforms.map((os) => { const c = f.platforms[os]; return c.implementation + ' · ' + c.verification; }), f.owner + ' / ' + f.issue.state].map(escaped).join(' | ') + ' |'),
    '', '## 증거 / 종료 조건', '',
    ...matrix.features.flatMap((f) => [
      '### ' + f.id + ' — ' + f.title, '',
      '- 추적: ' + f.issue.id + ' (' + f.issue.state + '), 담당: ' + f.owner,
      '- 잔여: ' + f.residual,
      '- 종료 조건: ' + f.acceptance,
      ...platforms.map((os) => {
        const c = f.platforms[os];
        return '- ' + os + ': ' + c.implementation + ' / ' + c.verification + ' (요구: ' + c.requiredVerification + '). ' +
          (c.note || '') + ' 근거: ' + c.evidence.map((e) => '[' + e.kind + ': ' + e.path + '](' + '../' + e.path + ')').join(', ');
      }), ''
    ]),
    '## Linux 배포판 / 환경', '',
    '| 환경 | 확인 수준 | 남은 검증 |', '|---|---|---|',
    ...matrix.linuxEnvironments.map((env) => '| ' + [env.name, env.status, env.residual].map(escaped).join(' | ') + ' |'),
    '', '## 플랫폼별 플러그인 형식', '',
    '| 형식 | macOS | Windows | Linux |', '|---|---|---|---|',
    ...matrix.pluginFormats.map((format) => '| ' + [format.format, ...platforms.map((os) => format[os])].join(' | ') + ' |'),
    '', '## 운용', '',
    '- 검증/문서 동기화: node scripts/check-feature-parity.mjs --check',
    '- 출시 게이트: node scripts/check-feature-parity.mjs --release (열린 P0/P1 존재 시 exit 1)',
    '- 게이트 논리 회귀: node scripts/check-feature-parity.mjs --self-test',
    '- owner는 작업 분담이고 완료 증거가 아닙니다. root가 결과를 검토하고 JSON의 상태·증거·잔여·issue를 함께 갱신합니다.',
    '- 이 파일의 추적 ID는 저장소 내부 관리 ID이며 외부 이슈 시스템에 생성됐다는 뜻이 아닙니다.', ''
  ];
  return text.join('\n');
}
if (args.has('--self-test')) {
  const assert = (value, label) => { if (!value) throw new Error('Self-test failed: ' + label); };
  const fixture = { schemaVersion: 1, features: [{
    id: 'fixture_gate', priority: 'P1', owner: 'root', acceptance: 'accept', residual: 'none',
    issue: { id: 'PARITY-fixture_gate', state: 'closed' },
    platforms: Object.fromEntries(platforms.map((os) => [os, {
      implementation: 'implemented', verification: 'unit', requiredVerification: 'native_device',
      evidence: [{ kind: 'source', path: 'package.json' }]
    }]))
  }]};
  validate(fixture);
  assert(openGates(fixture).length === 1, 'unit cannot satisfy native acceptance');
  for (const c of Object.values(fixture.features[0].platforms)) c.verification = 'native_device';
  assert(openGates(fixture).length === 0, 'closed implemented native gate');
  fixture.features[0].issue.state = 'open';
  assert(openGates(fixture).length === 1, 'open issue stays blocking');
  fixture.features[0].issue.state = 'closed';
  fixture.features[0].platforms.Windows.implementation = 'partial';
  assert(openGates(fixture).length === 1, 'partial stays blocking despite native test');
  fixture.features[0].platforms.Windows = { ...fixture.features[0].platforms.Windows, implementation: 'not_applicable', verification: 'source', rationale: 'platform-specific format' };
  assert(openGates(fixture).length === 0, 'inapplicable format is exempt');
  fixture.features.push(fixture.features[0]);
  let rejected = false;
  try { validate(fixture); } catch { rejected = true; }
  assert(rejected, 'duplicate IDs reject');
  console.log('Feature parity self-tests passed (6 assertions).');
  process.exit(0);
}
const matrix = JSON.parse(readFileSync(input, 'utf8'));
validate(matrix);
const expected = render(matrix);
if (args.has('--check') || args.has('--release')) {
  if (!existsSync(output) || readFileSync(output, 'utf8') !== expected) {
    console.error('Feature parity Markdown is stale. Run node scripts/check-feature-parity.mjs.');
    process.exit(1);
  }
} else writeFileSync(output, expected);
const gates = openGates(matrix);
console.log(matrix.features.length + ' feature groups; ' + gates.length + ' open release gates (' +
  gates.filter((f) => f.priority === 'P0').length + ' P0, ' + gates.filter((f) => f.priority === 'P1').length + ' P1).');
if (args.has('--release') && gates.length) {
  for (const f of gates) console.error(f.issue.id + ': ' + f.owner + ' — ' + f.residual);
  process.exitCode = 1;
}
