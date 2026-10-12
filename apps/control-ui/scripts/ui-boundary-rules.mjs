import { parse } from '@babel/parser';
import * as css from 'css-tree';

const controls = new Set(['button', 'input', 'select', 'textarea', 'option']);
const layoutComponents = new Set(['ApplicationStatusBar']);
const stylingProps = new Set([
  'style',
  'styles',
  'classNames',
  'vars',
  'unstyled',
  'renderRoot',
  'dangerouslySetInnerHTML',
]);
const placementProperties = new Set([
  'grid-column',
  'grid-row',
  'grid-area',
  'order',
  'z-index',
  'align-self',
  'justify-self',
]);
const libraryPattern = /^(?:@mantine\/|react-aria-components(?:$|\/)|@xyflow\/react(?:$|\/))/u;
const colorPattern = /#[\da-f]{3,8}\b|\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(/iu;
const layoutProperties = new Set([
  'display',
  'position',
  'top',
  'right',
  'bottom',
  'left',
  'inset',
  'z-index',
  'width',
  'height',
  'min-width',
  'min-height',
  'max-width',
  'max-height',
  'overflow',
  'overflow-x',
  'overflow-y',
  'overflow-wrap',
  'overscroll-behavior',
  'overscroll-behavior-x',
  'overscroll-behavior-y',
  'box-sizing',
  'visibility',
  'flex',
  'flex-direction',
  'flex-wrap',
  'flex-grow',
  'flex-shrink',
  'flex-basis',
  'align-items',
  'align-self',
  'justify-content',
  'justify-self',
  'place-items',
  'gap',
  'row-gap',
  'column-gap',
  'order',
  'grid',
  'grid-area',
  'grid-column',
  'grid-row',
  'grid-template-columns',
  'grid-template-rows',
  'grid-auto-flow',
  'grid-auto-rows',
  'margin',
  'margin-top',
  'margin-right',
  'margin-bottom',
  'margin-left',
  'margin-inline',
  'padding',
  'padding-top',
  'padding-right',
  'padding-bottom',
  'padding-left',
  'padding-inline',
]);

function visit(node, callback) {
  if (!node || typeof node !== 'object') return;
  if (typeof node.type === 'string' && callback(node) === false) return;
  const children = Object.entries(node)
    .filter(([key]) => !['loc', 'comments', 'tokens'].includes(key))
    .flatMap(([, value]) => (Array.isArray(value) ? value : [value]));
  for (const child of children) visit(child, callback);
}

function issue(violations, node, code, message) {
  violations.push({ line: node.loc?.start?.line ?? 1, code, message });
}

function checkLibrary(node, core, violations) {
  const source =
    node.source?.value ?? (node.type === 'CallExpression' ? node.arguments[0]?.value : undefined);
  if (!core && typeof source === 'string' && libraryPattern.test(source)) {
    issue(violations, node, 'library', 'UI library imports belong in Core.');
  }
  if (!core && node.type === 'ImportDeclaration' && /\/core\/.*\.css$/u.test(source)) {
    issue(
      violations,
      node,
      'selector',
      'Consume the Core component instead of importing its stylesheet.',
    );
  }
  if (
    !core &&
    node.type === 'ImportDeclaration' &&
    /\.css$/u.test(source) &&
    !source.endsWith('.module.css')
  ) {
    issue(violations, node, 'selector', 'Global styles belong in the Core theme.');
  }
}

function inlineLayout(attribute) {
  const expression = attribute.value?.expression;
  return (
    expression?.type === 'ObjectExpression' &&
    expression.properties.every((property) => {
      const name = property.key?.name ?? property.key?.value;
      const dashed = name?.replace(/[A-Z]/gu, (letter) => `-${letter.toLowerCase()}`);
      return (
        property.type === 'ObjectProperty' && !property.computed && layoutProperties.has(dashed)
      );
    })
  );
}

function expandedAttributes(attributes, bindings, seen = new Set()) {
  const result = [];
  for (const attribute of attributes) {
    if (attribute.type === 'JSXAttribute') {
      result.push(attribute);
      continue;
    }
    const argument = attribute.argument;
    if (argument?.type === 'Identifier' && !seen.has(argument.name)) {
      const next = new Set([...seen, argument.name]);
      result.push(
        ...expandedAttributes([{ argument: bindings.get(argument.name) }], bindings, next),
      );
    }
    if (argument?.type === 'ObjectExpression') {
      const members = argument.properties.map((property) =>
        property.type === 'ObjectProperty'
          ? {
              type: 'JSXAttribute',
              name: { name: property.key.name ?? property.key.value },
              value: { expression: property.value },
              loc: property.loc,
            }
          : property,
      );
      result.push(...expandedAttributes(members, bindings, seen));
    }
  }
  return result;
}

function checkElement(node, imports, bindings, violations, layoutBindings) {
  const name = node.name.name;
  const imported = imports.get(name);
  if (controls.has(name))
    issue(violations, node, 'control', 'Use a Core control; interactive DOM belongs in Core.');
  if (['style', 'link'].includes(name))
    issue(
      violations,
      node,
      'override',
      'Stylesheets are owned by the Core theme; consumers cannot inject them through JSX.',
    );
  for (const attribute of expandedAttributes(node.attributes, bindings)) {
    if (attribute.type !== 'JSXAttribute') continue;
    checkAppearance(attribute, name, imported, violations);
    if (attribute.name.name === 'className' && imported?.source.includes('/core/')) {
      if (layoutComponents.has(imported.name))
        recordLayout(attribute, imports, violations, layoutBindings);
      else
        issue(
          violations,
          attribute,
          'override',
          'Core component appearance cannot be replaced with a caller className.',
        );
    }
  }
}

function checkAppearance(attribute, name, imported, violations) {
  const prop = attribute.name.name;
  const allowedLayout =
    prop === 'style' &&
    /^[a-z]/u.test(name ?? '') &&
    !controls.has(name) &&
    inlineLayout(attribute);
  if (stylingProps.has(prop) && !allowedLayout) {
    issue(
      violations,
      attribute,
      'override',
      'Styling overrides belong in tokens / theme / the owning Core component.',
    );
  }
  if (
    /^(?:data-tone|data-size|data-variant|data-density|data-stretch)$/u.test(prop) &&
    imported?.source.includes('/core/')
  ) {
    issue(
      violations,
      attribute,
      'override',
      'Select the Core semantic prop instead of overriding its internal state attributes.',
    );
  }
  if (
    prop === 'contentEditable' ||
    (prop === 'role' && controls.has(attribute.value?.value ?? attribute.value?.expression?.value))
  ) {
    issue(violations, attribute, 'control', 'Editable and interactive roles belong in Core.');
  }
}

function recordLayout(attribute, imports, violations, layoutBindings) {
  const value = attribute.value?.expression;
  const imported = imports.get(value?.object?.name);
  if (
    value?.type !== 'MemberExpression' ||
    value.computed ||
    !imported?.source.endsWith('.module.css')
  ) {
    issue(
      violations,
      attribute,
      'layout',
      'Core layout className must reference one local CSS module class.',
    );
    return;
  }
  layoutBindings.push({ source: imported.source, name: value.property.name });
}

export function inspectScript(filename, source) {
  const core = filename.startsWith('core/');
  const test = /\.test\.tsx?$/u.test(filename);
  const violations = [];
  const layoutBindings = [];
  const ast = parse(source, { sourceType: 'module', plugins: ['typescript', 'jsx'] });
  const imports = new Map();
  const bindings = new Map();
  visit(ast, (node) => {
    if (node.type === 'VariableDeclarator' && node.id.type === 'Identifier')
      bindings.set(node.id.name, node.init);
    if (node.type === 'ImportDeclaration') {
      for (const specifier of node.specifiers)
        imports.set(specifier.local.name, {
          source: node.source.value,
          name: specifier.imported?.name ?? specifier.local.name,
        });
    }
  });
  if (filename.startsWith('core/ui/') && !test && !filename.includes('.stories.')) {
    checkCoreApi(ast, imports, violations);
  }
  visit(ast, (node) => {
    if (
      [
        'ImportDeclaration',
        'ExportNamedDeclaration',
        'ExportAllDeclaration',
        'CallExpression',
        'ImportExpression',
      ].includes(node.type)
    )
      checkLibrary(node, core, violations);
    if (
      !test &&
      filename !== 'core/theme/tokens.ts' &&
      ['StringLiteral', 'TemplateElement'].includes(node.type) &&
      colorPattern.test(node.value?.raw ?? node.value)
    ) {
      issue(violations, node, 'color', 'Define color literals in core/theme/tokens.ts.');
    }
    if (!core && !test && node.type === 'JSXOpeningElement')
      checkElement(node, imports, bindings, violations, layoutBindings);
  });
  return { violations, layoutBindings };
}

function checkCoreApi(ast, imports, violations) {
  visit(ast, (node) => {
    if (node.type !== 'TSTypeReference') return;
    const imported = imports.get(node.typeName.name);
    if (imported?.source.startsWith('@mantine/') && imported.name.endsWith('Props')) {
      issue(
        violations,
        node,
        'api',
        'Core must expose an explicit product contract instead of all library props.',
      );
    }
    if (node.typeName.name === 'Pick') {
      const keys = node.typeParameters?.params[1];
      visit(keys, (key) => {
        if (
          key.type === 'StringLiteral' &&
          [...stylingProps, 'className', 'color', 'c', 'size', 'radius', 'variant'].includes(
            key.value,
          )
        ) {
          issue(
            violations,
            key,
            'api',
            'Do not expose library appearance props through the Core contract.',
          );
        }
      });
      return false;
    }
  });
}

function checkSelector(node, violations) {
  css.walk(node.prelude, (part) => {
    if (
      part.type === 'TypeSelector' &&
      (controls.has(part.name.toLowerCase()) ||
        ['html', 'body', '*'].includes(part.name.toLowerCase()))
    ) {
      issue(
        violations,
        part,
        'selector',
        'Consumer CSS cannot target controls or global elements.',
      );
    }
    if (
      part.type === 'PseudoClassSelector' &&
      ['root', 'global'].includes(part.name.toLowerCase())
    ) {
      issue(violations, part, 'selector', 'Global / library / Core selectors belong in Core.');
    }
    if (
      part.type === 'ClassSelector' &&
      /^(?:mantine-|react-flow|action-button)/u.test(part.name)
    ) {
      issue(
        violations,
        part,
        'selector',
        'Library / Core internals cannot be styled by consumers.',
      );
    }
    if (part.type === 'AttributeSelector') checkAttributeSelector(part, violations);
  });
}

function checkAttributeSelector(part, violations) {
  if (
    /^(?:class|style|data-tone|data-size|data-variant|data-density|data-stretch)$/u.test(
      part.name.name,
    )
  ) {
    issue(
      violations,
      part,
      'selector',
      'Core implementation attributes cannot be styled by consumers.',
    );
  }
  if (
    part.name.name === 'role' &&
    /^(?:button|checkbox|switch|slider|option|menuitem|textbox|combobox)$/u.test(
      part.value?.value ?? part.value?.name,
    )
  ) {
    issue(
      violations,
      part,
      'selector',
      'Interactive roles are styled by their owning Core component.',
    );
  }
}

function checkDeclaration(node, core, violations, atrule) {
  if (isCoreFontDescriptor(core, atrule)) return;
  const value = css.generate(node.value);
  if (!core && node.important)
    issue(
      violations,
      node,
      'important',
      'Change the owning contract instead of adding !important.',
    );
  if (!core && node.property.startsWith('--') && !node.property.startsWith('--layout-')) {
    issue(
      violations,
      node,
      'token',
      'Visual tokens are defined in Core; only --layout-* variables may be local.',
    );
  }
  if (!layoutProperties.has(node.property) && /var\(--layout-/u.test(value)) {
    issue(violations, node, 'token', 'Layout variables cannot replace visual role tokens.');
  }
  if (
    ['font-size', 'font-family'].includes(node.property) &&
    !/^(?:var\(--[\w-]+\)|inherit)$/u.test(value)
  ) {
    issue(violations, node, 'typography', 'Use a typography role from core/theme/tokens.ts.');
  }
  if (colorPattern.test(value))
    issue(violations, node, 'color', 'Define color literals in core/theme/tokens.ts.');
  css.walk(node.value, (part) => {
    if (
      part.type === 'Identifier' &&
      !['transparent', 'currentcolor', 'inherit'].includes(part.name.toLowerCase()) &&
      css.lexer.matchType('color', part.name).matched
    ) {
      issue(violations, node, 'color', 'Named colors also belong in core/theme/tokens.ts.');
    }
  });
}

function isCoreFontDescriptor(core, atrule) {
  return core && atrule?.name === 'font-face';
}

export function inspectCss(filename, source, layoutClasses = new Set()) {
  const core = filename.startsWith('core/');
  const violations = [];
  const ast = css.parse(source, { positions: true });
  css.walk(ast, function inspect(node) {
    if (!core && node.type === 'Atrule' && node.name === 'import')
      issue(violations, node, 'selector', 'Consumer CSS cannot import global styles.');
    if (node.type === 'Rule' && !core) checkSelector(node, violations);
    if (node.type !== 'Declaration') return;
    // Font-face descriptors declare the bundled family; they are not consumer typography.
    checkDeclaration(node, core, violations, this.atrule);
    if (!core && this.rule?.prelude) {
      let layout = false;
      css.walk(this.rule.prelude, (part) => {
        if (part.type === 'ClassSelector' && layoutClasses.has(part.name)) layout = true;
      });
      if (layout && !placementProperties.has(node.property)) {
        issue(
          violations,
          node,
          'layout',
          'Core container className may only set outer placement; sizing and internal spacing belong in Core.',
        );
      }
    }
  });
  return violations;
}
