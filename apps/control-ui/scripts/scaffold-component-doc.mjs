#!/usr/bin/env node
/**
 * Writes the standard Design System component page for a registered contract.
 *
 *   pnpm --filter @mixrelay/control-ui docs:page <ComponentName> [--force | --stdout]
 *
 * The page follows the shared template: contract summary, Playground with the real
 * props table, the existing usage story named by the contract and one Canvas per
 * playground state story. Prose stays in componentContracts.ts so it is written once.
 */
import { existsSync } from 'node:fs';
import { readdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import { parse } from '@babel/parser';
import { sanitize, storyNameFromExport, toId } from 'storybook/internal/csf';
import { componentContracts } from '../src/core/design-system/componentContracts.ts';

const appRoot = path.resolve(import.meta.dirname, '..');
const designSystem = path.join(appRoot, 'src/core/design-system');
const excludedProps = [
  'className',
  'classNames',
  'style',
  'styles',
  'vars',
  'unstyled',
  'color',
  'c',
  'size',
  'radius',
  'variant',
  'component',
  'renderRoot',
  'dangerouslySetInnerHTML',
];

async function storyFiles(directory) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const filename = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...(await storyFiles(filename)));
    else if (entry.name.endsWith('.stories.tsx')) files.push(filename);
  }
  return files;
}

const typeWrappers = new Set(['TSSatisfiesExpression', 'TSAsExpression']);

function unwrapType(node) {
  let current = node;
  while (current && typeWrappers.has(current.type)) current = current.expression;
  return current;
}

/** The CSF title: `export default {title}` or the object bound to the default export. */
function csfTitle(program) {
  const objects = new Map();
  for (const statement of program.body)
    if (statement.type === 'VariableDeclaration')
      for (const declaration of statement.declarations)
        if (declaration.id.type === 'Identifier')
          objects.set(declaration.id.name, declaration.init);
  const exported = program.body.find((statement) => statement.type === 'ExportDefaultDeclaration');
  let meta = unwrapType(exported?.declaration);
  if (meta?.type === 'Identifier') meta = unwrapType(objects.get(meta.name));
  const title = meta?.properties?.find(
    (property) => property.key?.name === 'title' || property.key?.value === 'title',
  )?.value;
  return title?.type === 'StringLiteral' ? title.value : undefined;
}

function csfExports(program) {
  return program.body.flatMap((statement) =>
    statement.type === 'ExportNamedDeclaration' &&
    statement.declaration?.type === 'VariableDeclaration'
      ? statement.declaration.declarations.map((declaration) => declaration.id.name)
      : [],
  );
}

/** Find the CSF file and export behind a story id using Storybook's own id rules. */
async function storyExport(id, files) {
  for (const filename of files) {
    const program = parse(await readFile(filename, 'utf8'), {
      sourceType: 'module',
      plugins: ['typescript', 'jsx'],
    }).program;
    const title = csfTitle(program);
    if (!title || !id.startsWith(`${sanitize(title)}--`)) continue;
    const name = csfExports(program).find((key) => toId(title, storyNameFromExport(key)) === id);
    if (name) return { filename, name };
  }
  throw new Error(`No story file exports ${id}`);
}

function relativeModule(filename) {
  return path
    .relative(path.join(designSystem, 'components'), filename)
    .replace(/\.tsx$/u, '')
    .split(path.sep)
    .join('/');
}

async function page(contract, previous = '') {
  // Regenerating keeps a hand-tuned Playground canvas height.
  const height =
    previous.match(/of=\{Api\.\w+\} story=\{\{ inline: false, height: (\d+) \}\}/u)?.[1] ?? '320';
  const files = await storyFiles(path.join(appRoot, 'src'));
  const playground = await storyExport(contract.playgroundStory, files);
  const example = await storyExport(contract.storyId, files);
  const playgroundModule = relativeModule(playground.filename);
  const states = [];
  for (const state of contract.stateStories ?? []) {
    const story = await storyExport(state.id, files);
    if (story.filename === playground.filename) states.push({ ...state, name: story.name });
  }
  const exclude = JSON.stringify(excludedProps);
  const playgroundSource = await readFile(playground.filename, 'utf8');
  // The playground's docs description is the one-line behaviour note shown above the canvas.
  const description = playgroundSource
    .match(/component:\s*'((?:[^'\\]|\\.)*)'/u)?.[1]
    ?.replace(/\\'/gu, "'");
  return `import { Meta, Canvas, ArgTypes } from '@storybook/addon-docs/blocks';
import { DocsLayout } from '../DocsLayout';
import { ComponentContract } from '../ComponentsReference';
import * as Example from '${relativeModule(example.filename)}';

import * as Api from "${playgroundModule}";

<Meta title="Design System/Components/${contract.name}" />

<DocsLayout>

<ComponentContract name="${contract.name}" />

## Playground·API·기본값

<a href="/?path=/story/${contract.playgroundStory}" target="_top">${contract.name} Playground 열기</a>. Controls는 실제 공개 props를 전달합니다. 구현 기본값은 아래 API 표와 소스 대조 계약을 따르며, Playground의 args는 재현용 입력입니다.

${description ? `${description}\n\n` : ''}<Canvas of={Api.${playground.name}} story={{ inline: false, height: ${height} }} sourceState="hidden" />
<ArgTypes of={Api.${playground.name}} exclude={${exclude}} />

API 표의 *는 필수 입력입니다. Default의 —는 선언된 기본값이 없다는 뜻이며, 파생값·fallback과 초기 상태는 위 소스 대조 계약에 명시합니다.

속성은 Playground의 Controls에서 변경합니다. Source는 현재 JSX를, 위의 실행 예제 전체 코드는 실제 상태 갱신·이벤트·슬롯 구성을 보여줍니다.

## 실제 사용례

이 Canvas는 아래 계약을 소비하는 기존 스토리입니다. 여러 컴포넌트의 조합이면 해당 조합에서의 역할을 함께 확인합니다. 독립 컴포넌트 전용 예시나 실제 장치 실행으로 해석하지 않습니다.

<Canvas of={Example.${example.name}} story={{ inline: false, height: 560 }} />
${
  states.length
    ? `
## 개별 상태 예제
${states
  .map(
    (state) => `
### ${state.label.replace(/^.*\/\s*/u, '')}

<Canvas of={Api.${state.name}} story={{ inline: false, height: 400 }} sourceState="hidden" />
`,
  )
  .join('')}`
    : ''
}
<a href="/?path=/docs/design-system-components--docs" target="_top">전체 컴포넌트와 사용 구분</a>

</DocsLayout>
`;
}

const usage = `Usage: pnpm --filter @mixrelay/control-ui docs:page <ComponentName> [--force | --stdout]
Writes src/core/design-system/components/<ComponentName>.mdx from the registered contract.
  --stdout  print the page instead of writing it (no files are created)
  --force   overwrite an existing page, keeping its Playground canvas height
Fails when the contract or any referenced story is missing.`;
if (process.argv.includes('--help')) {
  console.log(usage);
  process.exit(0);
}
const [name] = process.argv.slice(2).filter((argument) => !argument.startsWith('--'));
const contract = componentContracts.find((entry) => entry.name === name);
if (!contract) {
  console.error(`Register ${name ?? '<name>'} in componentContracts.ts first.`);
  process.exit(1);
}
const target = path.join(designSystem, 'components', `${contract.name}.mdx`);
if (process.argv.includes('--stdout')) {
  process.stdout.write(
    await page(contract, existsSync(target) ? await readFile(target, 'utf8') : ''),
  );
  process.exit(0);
}
if (existsSync(target) && !process.argv.includes('--force')) {
  console.error(`${path.relative(appRoot, target)} exists; pass --force to regenerate it.`);
  process.exit(1);
}
const previous = existsSync(target) ? await readFile(target, 'utf8') : '';
await writeFile(target, await page(contract, previous));
console.log(`Wrote ${path.relative(appRoot, target)}`);
