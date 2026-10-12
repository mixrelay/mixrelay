import assert from 'node:assert/strict';
import { test } from 'node:test';
import { inspectCss, inspectScript } from './ui-boundary-rules.mjs';

const script = (source) => inspectScript('features/example/Example.tsx', source).violations;
const css = (source, classes) => inspectCss('features/example/Example.module.css', source, classes);
const has = (items, code) => items.some((item) => item.code === code);

for (const source of [
  "import { Button } from '@mantine/core';",
  "import { Button } from 'react-aria-components';",
  "const library = import('react-aria-components/Menu');",
  "import '@mantine/core/styles.css';",
  "export { ReactFlow } from '@xyflow/react';",
  "const library = import('@mantine/core');",
  "const library = require('@xyflow/react');",
]) {
  test(`blocks direct UI library access: ${source}`, () =>
    assert.ok(has(script(source), 'library')));
}

test('Core may adapt external libraries; consumer semantic props and action spreads are allowed', () => {
  assert.deepEqual(
    inspectScript(
      'core/ui/Control.tsx',
      "import { Button } from '@mantine/core'; export const C = () => <Button styles={{root: {color: 'var(--text)'}}} />;",
    ).violations,
    [],
  );
  assert.deepEqual(
    script(
      "import { ActionButton as Command } from '../../core/ui/ActionButton'; const C = () => <Command tone='critical' density='compact' emphasis='outlined' {...actions}>Remove</Command>;",
    ),
    [],
  );
});

for (const source of [
  "import { ActionButton as Command } from '../../core/ui/ActionButton'; const C = () => <Command className='override' />;",
  "import { TextField } from '../../core/ui/TextField'; const C = () => <TextField classNames={{input: 'override'}} />;",
  "const C = () => <div style={{fontSize: 'var(--font-control)'}} />;",
  "import { ActionButton } from '../../core/ui/ActionButton'; const C = () => <ActionButton data-tone='critical' />;",
]) {
  test(`blocks caller appearance: ${source}`, () => assert.ok(has(script(source), 'override')));
}

for (const tag of ['button', 'input', 'select', 'textarea']) {
  test(`requires Core to own native ${tag}`, () =>
    assert.ok(has(script(`const C = () => <${tag} />;`), 'control')));
}

test('test fixtures may exercise native DOM, but stories follow consumer rules', () => {
  assert.deepEqual(
    inspectScript(
      'features/example/Example.test.tsx',
      "const fixture = <button style={{color:'#fff'}} />;",
    ).violations,
    [],
  );
  assert.ok(
    has(
      inspectScript('features/example/Example.stories.tsx', 'const C = () => <button />;')
        .violations,
      'control',
    ),
  );
});

test('inline story geometry and local layout variables do not require a visual token', () => {
  assert.deepEqual(
    script("const C = () => <div style={{height:'100dvh', minHeight:320, padding:24}} />;"),
    [],
  );
  assert.deepEqual(
    css(
      '.root { --layout-sidebar-width: 200px; display:grid; width:var(--layout-sidebar-width); color:var(--text); font-size:var(--font-body); }',
    ),
    [],
  );
});

test('layout container bindings are checked against the associated CSS module', () => {
  const source =
    "import { ApplicationStatusBar as Header } from '../../core/ui/ApplicationStatusBar'; import s from './Example.module.css'; const C=()=> <Header className={s.toolbar} />;";
  const result = inspectScript('features/example/Example.tsx', source);
  assert.deepEqual(result.violations, []);
  assert.deepEqual(result.layoutBindings, [{ source: './Example.module.css', name: 'toolbar' }]);
  assert.deepEqual(css('.toolbar { z-index:6; grid-row:3; }', new Set(['toolbar'])), []);
  assert.ok(
    has(css('.toolbar { color:var(--red); height:24px; }', new Set(['toolbar'])), 'layout'),
  );
  assert.ok(
    has(
      script(
        "import { ApplicationStatusBar } from '../../core/ui/ApplicationStatusBar'; const C=()=> <ApplicationStatusBar className={dynamic} />;",
      ),
      'layout',
    ),
  );
});

for (const source of [
  '.root :global(.mantine-Button-label) { color:var(--red); }',
  '.root button { color:var(--text); }',
  ':root { --text:var(--red); }',
  '.wrapper [role="button"] { color:var(--red); }',
  '.root [data-variant="outline"] { border:0; }',
  '.root .action-button { padding:0; }',
  '.root * { font-size:var(--font-caption); }',
  '@import "./override.css";',
]) {
  test(`blocks consumer selector escapes: ${source}`, () =>
    assert.ok(has(css(source), 'selector')));
}

test('tokens cannot be locally redeclared or disguised as layout variables', () => {
  assert.ok(has(css('.root { --control-height:24px; }'), 'token'));
  assert.ok(has(css('.root { --special-button-color:var(--red); }'), 'token'));
  assert.ok(has(css('.root { --layout-color:red; color:var(--layout-color); }'), 'token'));
  assert.ok(has(css('.root { color:var(--text) !important; }'), 'important'));
});

test('typography and all color forms use system roles, including named colors', () => {
  for (const value of ['#fff', 'rgb(255 0 0)', 'oklch(50% 0.2 20)', 'rebeccapurple'])
    assert.ok(has(css(`.root { color:${value}; }`), 'color'));
  assert.ok(has(css('.root { font-size:12px; }'), 'typography'));
  assert.ok(has(css('.root { font-family:Arial; }'), 'typography'));
  assert.deepEqual(
    css(
      '.root { color:currentColor; background:transparent; font-size:var(--font-control); font-family:inherit; }',
    ),
    [],
  );
  assert.ok(has(script("const color = '#fff';"), 'color'));
  assert.deepEqual(inspectScript('core/theme/tokens.ts', "const color = '#fff';").violations, []);
});

test('consumer imports cannot bypass Core by loading its stylesheet', () => {
  assert.ok(has(script("import '../../core/ui/ActionButton.module.css';"), 'selector'));
  assert.ok(has(script("import './override.css';"), 'selector'));
});

test('static object spreads cannot hide caller overrides, including aliases and nested spreads', () => {
  const source =
    "import { ActionButton } from '../../core/ui/ActionButton'; const base = {styles: {root: {color:'var(--red)'}}}; const alias = base; const props = {...alias, disabled:false}; const C=()=> <ActionButton {...props} />;";
  assert.ok(has(script(source), 'override'));
  assert.ok(
    has(script("const C=()=> <div {...{style:{fontSize:'var(--font-control)'}}} />;"), 'override'),
  );
  assert.deepEqual(
    script('const props = {disabled:true}; const C=()=> <Feature {...props} />;'),
    [],
  );
});

test('Core APIs cannot re-export the full library styling surface', () => {
  const broad =
    "import type {ButtonProps as LibraryProps} from '@mantine/core'; export function ActionButton(props: LibraryProps) { return null; }";
  assert.ok(has(inspectScript('core/ui/ActionButton.tsx', broad).violations, 'api'));
  const narrow =
    "import type {TextInputProps} from '@mantine/core'; type Props = Pick<TextInputProps, 'label' | 'onChange'>;";
  assert.deepEqual(inspectScript('core/ui/TextField.tsx', narrow).violations, []);
  assert.ok(
    has(
      inspectScript('core/ui/TextField.tsx', narrow.replace("'label'", "'styles'")).violations,
      'api',
    ),
  );
});

test('consumer JSX cannot inject global style sheets or replace a control render root', () => {
  assert.ok(has(script("const C=()=> <style>{'.action-button {padding:0}'}</style>;"), 'override'));
  assert.ok(
    has(
      script(
        "const C=()=> <div dangerouslySetInnerHTML={{__html:'<style>.action-button{padding:0}</style>'}} />;",
      ),
      'override',
    ),
  );
  assert.ok(has(script('const C=()=> <Feature renderRoot={customRoot} />;'), 'override'));
});

test('Core can declare bundled font faces while consumer typography still requires roles', () => {
  const face =
    "@font-face { font-family: 'IBM Plex Sans'; src: url('./fonts/Plex.woff2'); font-weight: 400; }";
  assert.deepEqual(inspectCss('core/theme/Fonts.css', face), []);
  assert.ok(has(css(face), 'typography'));
  assert.ok(has(inspectCss('core/theme/Bad.css', "p { font-family: 'Other'; }"), 'typography'));
});
