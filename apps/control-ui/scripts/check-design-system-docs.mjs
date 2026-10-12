import assert from 'node:assert/strict';
import { readdir, readFile, stat } from 'node:fs/promises';
import path from 'node:path';
import { parse } from '@babel/parser';
import { componentContracts } from '../src/core/design-system/componentContracts.ts';
import { pageContracts } from '../src/core/design-system/pageContracts.ts';

const appRoot = path.resolve(import.meta.dirname, '..');
const repositoryRoot = path.resolve(appRoot, '../..');
const docsRoot = path.join(appRoot, 'src/core/design-system');
const parserOptions = { sourceType: 'module', plugins: ['typescript', 'jsx'] };
const ignoredAstKeys = new Set([
  'start',
  'end',
  'loc',
  'extra',
  'leadingComments',
  'innerComments',
  'trailingComments',
  'comments',
  'tokens',
  'errors',
]);

// Compare syntax that owns the public contract, never component implementation bodies.
// Member/union order and formatting do not change the TypeScript contract.
function normalize(value) {
  if (Array.isArray(value)) return value.map(normalize);
  if (!value || typeof value !== 'object') return value;
  const result = {};
  for (const key of Object.keys(value).sort()) {
    if (ignoredAstKeys.has(key) || value[key] === undefined || value[key] === null) continue;
    result[key] = normalize(value[key]);
  }
  const sortable =
    value.type === 'TSUnionType' || value.type === 'TSIntersectionType'
      ? 'types'
      : value.type === 'TSTypeLiteral'
        ? 'members'
        : value.type === 'TSInterfaceBody'
          ? 'body'
          : undefined;
  if (sortable) result[sortable].sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
  return result;
}
const fingerprint = (value) => JSON.stringify(normalize(value));

function walk(node, visitor) {
  if (!node || typeof node !== 'object') return;
  if (Array.isArray(node)) {
    for (const child of node) walk(child, visitor);
    return;
  }
  visitor(node);
  for (const [key, child] of Object.entries(node)) {
    if (!ignoredAstKeys.has(key)) walk(child, visitor);
  }
}
function parseModule(source, filename = 'source') {
  try {
    return parse(source, parserOptions).program;
  } catch (error) {
    throw new Error(`${filename}: ${error.message}`);
  }
}
function parseApi(source) {
  // Existing API excerpts intentionally omit the final function implementation.
  try {
    return parseModule(source, 'API excerpt');
  } catch {
    return parseModule(`${source}\n{}`, 'API excerpt');
  }
}
function recordImports(statement, info) {
  if (statement.type !== 'ImportDeclaration') return;
  for (const specifier of statement.specifiers) {
    const imported =
      specifier.type === 'ImportSpecifier'
        ? (specifier.imported.name ?? specifier.imported.value)
        : specifier.type === 'ImportDefaultSpecifier'
          ? 'default'
          : '*';
    info.imports.set(specifier.local.name, { source: statement.source.value, imported });
  }
}
function declarationNames(declaration) {
  if (declaration?.id?.name) return [declaration.id.name];
  if (declaration?.type !== 'VariableDeclaration') return [];
  return declaration.declarations
    .filter((item) => item.id.type === 'Identifier')
    .map((item) => item.id.name);
}
function recordBindings(declaration, info) {
  if (declaration?.id?.name) {
    info.bindings.set(declaration.id.name, declaration);
    if (['TSTypeAliasDeclaration', 'TSInterfaceDeclaration'].includes(declaration.type)) {
      info.types.set(declaration.id.name, declaration);
    }
  }
  if (declaration?.type !== 'VariableDeclaration') return;
  for (const item of declaration.declarations) {
    if (item.id.type !== 'Identifier') continue;
    info.bindings.set(item.id.name, item.init);
    if (item.id.typeAnnotation) info.annotations.set(item.id.name, item.id.typeAnnotation);
  }
}
function recordExports(statement, declaration, info) {
  if (statement.type === 'ExportDefaultDeclaration') {
    const name =
      declaration?.id?.name ?? (declaration?.type === 'Identifier' ? declaration.name : undefined);
    if (name) info.exports.set('default', name);
    else info.anonymousDefault = declaration;
  }
  if (statement.type !== 'ExportNamedDeclaration') return;
  for (const name of declarationNames(declaration)) info.exports.set(name, name);
  if (statement.source) return; // A barrel is not the declaration's source owner.
  for (const specifier of statement.specifiers) {
    if (specifier.type === 'ExportSpecifier') {
      info.exports.set(specifier.exported.name ?? specifier.exported.value, specifier.local.name);
    }
  }
}
function moduleInfo(program) {
  const info = {
    bindings: new Map(),
    annotations: new Map(),
    types: new Map(),
    imports: new Map(),
    exports: new Map(),
  };
  for (const statement of program.body) {
    recordImports(statement, info);
    const declaration = ['ExportNamedDeclaration', 'ExportDefaultDeclaration'].includes(
      statement.type,
    )
      ? statement.declaration
      : statement;
    recordBindings(declaration, info);
    recordExports(statement, declaration, info);
  }
  return info;
}
function reactCallName(node, info) {
  if (node?.type === 'Identifier') {
    const imported = info.imports.get(node.name);
    if (imported?.source === 'react') return imported.imported;
    // API excerpts can use the standard wrapper names without repeating imports.
    if (
      !info.bindings.has(node.name) &&
      ['memo', 'forwardRef', 'createElement'].includes(node.name)
    ) {
      return node.name;
    }
  }
  if (node?.type === 'MemberExpression' && !node.computed && node.object.type === 'Identifier') {
    const imported = info.imports.get(node.object.name);
    if (imported?.source === 'react' || node.object.name === 'React') return node.property.name;
  }
}
function callable(node, info, seen = new Set()) {
  if (!node) return undefined;
  if (
    [
      'FunctionDeclaration',
      'FunctionExpression',
      'ArrowFunctionExpression',
      'TSDeclareFunction',
    ].includes(node.type)
  ) {
    return { fn: node, wrappers: [] };
  }
  if (node.type === 'Identifier' && !seen.has(node.name)) {
    seen.add(node.name);
    return callable(info.bindings.get(node.name), info, seen);
  }
  if (
    node.type === 'CallExpression' &&
    ['memo', 'forwardRef'].includes(reactCallName(node.callee, info))
  ) {
    const inner = callable(node.arguments[0], info, seen);
    if (inner)
      return {
        fn: inner.fn,
        wrappers: [
          {
            name: reactCallName(node.callee, info),
            typeParameters: node.typeParameters ?? node.typeArguments,
          },
          ...inner.wrappers,
        ],
      };
  }
}
function signature(call) {
  return {
    params: call.fn.params,
    typeParameters: call.fn.typeParameters,
    returnType: call.fn.returnType,
    async: !!call.fn.async,
    generator: !!call.fn.generator,
    wrappers: call.wrappers,
    declarationType: call.declarationType,
  };
}
function namedCallable(name, info) {
  const call = callable(info.bindings.get(name), info);
  return call && { ...call, declarationType: info.annotations.get(name) };
}
function walkOwnRender(node, visitor) {
  if (!node || typeof node !== 'object') return;
  if (Array.isArray(node)) {
    for (const child of node) walkOwnRender(child, visitor);
    return;
  }
  // A factory containing a render callback is not itself a React component.
  if (
    [
      'FunctionDeclaration',
      'FunctionExpression',
      'ArrowFunctionExpression',
      'ClassDeclaration',
      'ClassExpression',
    ].includes(node.type)
  )
    return;
  visitor(node);
  for (const [key, child] of Object.entries(node)) {
    if (!ignoredAstKeys.has(key)) walkOwnRender(child, visitor);
  }
}
function qualifiedTypeName(node) {
  if (node.type === 'Identifier') return node.name;
  if (node.type === 'TSQualifiedName')
    return `${qualifiedTypeName(node.left)}.${qualifiedTypeName(node.right)}`;
}
function isComponent(call, info) {
  if (!call) return false;
  if (call.wrappers.length) return true;
  let renders = false;
  walkOwnRender(call.fn.body, (node) => {
    if (
      node.type === 'JSXElement' ||
      node.type === 'JSXFragment' ||
      (node.type === 'CallExpression' && reactCallName(node.callee, info) === 'createElement')
    )
      renders = true;
  });
  walk(call.fn.returnType, (node) => {
    if (node.type === 'Identifier' && ['ReactNode', 'ReactElement'].includes(node.name))
      renders = true;
    if (
      node.type === 'TSQualifiedName' &&
      ['JSX.Element', 'React.JSX.Element'].includes(qualifiedTypeName(node))
    )
      renders = true;
  });
  return renders;
}
function exportedComponents(info) {
  const result = [];
  for (const [exported, local] of info.exports) {
    const name = exported === 'default' ? local : exported;
    const call = namedCallable(local, info);
    if (/^[A-Z]/u.test(name) && isComponent(call, info)) result.push({ name, local, call });
  }
  if (isComponent(callable(info.anonymousDefault, info), info))
    result.push({ name: '(anonymous default)' });
  return result;
}
function typeReferenceName(node) {
  if (node.type === 'TSTypeReference' && node.typeName.type === 'Identifier')
    return node.typeName.name;
  if (
    ['TSExpressionWithTypeArguments', 'TSInterfaceHeritage'].includes(node.type) &&
    node.expression?.type === 'Identifier'
  )
    return node.expression.name;
}
function genericScope(node, parent) {
  const parameters = node.typeParameters?.params ?? [];
  if (!parameters.length && node.type !== 'TSMappedType') return parent;
  const scope = new Set(parent);
  for (const parameter of parameters) scope.add(parameter.name);
  if (node.type === 'TSMappedType') scope.add(node.typeParameter.name);
  return scope;
}
function visitTypeReferences(node, scope, names) {
  if (!node || typeof node !== 'object') return;
  if (Array.isArray(node)) {
    for (const item of node) visitTypeReferences(item, scope, names);
    return;
  }
  scope = genericScope(node, scope);
  const name = typeReferenceName(node);
  if (name && !scope.has(name)) names.add(name);
  for (const [key, value] of Object.entries(node)) {
    if (!ignoredAstKeys.has(key)) visitTypeReferences(value, scope, names);
  }
}
function typeReferences(node) {
  const names = new Set();
  visitTypeReferences(node, new Set(), names);
  return names;
}
function localTypeClosure(info, call) {
  const required = new Map();
  const pending = [...typeReferences(signature(call))];
  while (pending.length) {
    const name = pending.pop();
    const declaration = info.types.get(name);
    if (!declaration || required.has(name)) continue;
    required.set(name, declaration);
    pending.push(...typeReferences(declaration));
  }
  return required;
}
function compareDefaultConstants(name, call, sourceInfo, apiInfo) {
  const failures = [];
  const dependencies = new Set();
  walk(call.fn.params, (node) => {
    if (node.type === 'AssignmentPattern' && node.right.type === 'Identifier') {
      dependencies.add(node.right.name);
    }
  });
  for (const dependency of dependencies) {
    const source = sourceInfo.bindings.get(dependency);
    if (!source || callable(source, sourceInfo)) continue;
    const documented = apiInfo.bindings.get(dependency);
    if (!documented)
      failures.push(`${name}: API excerpt missing source-local default constant ${dependency}`);
    else if (
      fingerprint(source) !== fingerprint(documented) ||
      fingerprint(sourceInfo.annotations.get(dependency)) !==
        fingerprint(apiInfo.annotations.get(dependency))
    ) {
      failures.push(`${name}: default constant ${dependency} differs from source`);
    }
  }
  return failures;
}
function compareApi(name, sourceInfo, apiInfo) {
  const failures = [];
  const sourceLocal =
    sourceInfo.exports.get(name) ?? (sourceInfo.exports.get('default') === name ? name : undefined);
  const sourceCall = namedCallable(sourceLocal, sourceInfo);
  const apiLocal = apiInfo.exports.get(name) ?? name;
  const apiCall = namedCallable(apiLocal, apiInfo);
  if (!sourceCall) return [`${name}: source does not export a callable component with this name`];
  if (!apiCall) return [`${name}: API excerpt has no callable signature`];
  if (fingerprint(signature(sourceCall)) !== fingerprint(signature(apiCall))) {
    failures.push(`${name}: API signature/defaults differ from source`);
  }
  failures.push(...compareDefaultConstants(name, sourceCall, sourceInfo, apiInfo));
  for (const [typeName, sourceType] of localTypeClosure(sourceInfo, sourceCall)) {
    const apiType = apiInfo.types.get(typeName);
    if (!apiType) failures.push(`${name}: API excerpt missing source-local type ${typeName}`);
    else if (fingerprint(sourceType) !== fingerprint(apiType)) {
      failures.push(`${name}: API type ${typeName} differs from source`);
    }
  }
  return failures;
}

async function sourceFiles(directory) {
  const result = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const filename = path.join(directory, entry.name);
    if (entry.isDirectory()) result.push(...(await sourceFiles(filename)));
    else if (/\.tsx?$/u.test(entry.name) && !/\.(stories|test|spec)\.tsx?$/u.test(entry.name))
      result.push(filename);
  }
  return result;
}
function createModuleReader() {
  const modules = new Map();
  return async (filename) => {
    if (!modules.has(filename)) {
      modules.set(
        filename,
        moduleInfo(
          parseModule(await readFile(filename, 'utf8'), path.relative(repositoryRoot, filename)),
        ),
      );
    }
    return modules.get(filename);
  };
}
async function resolveImport(owner, specifier) {
  if (!specifier.startsWith('.')) return undefined;
  const base = path.resolve(path.dirname(owner), specifier);
  const candidates = [
    base,
    base.replace(/\.[cm]?jsx?$/u, '.ts'),
    base.replace(/\.[cm]?jsx?$/u, '.tsx'),
    `${base}.ts`,
    `${base}.tsx`,
    path.join(base, 'index.ts'),
    path.join(base, 'index.tsx'),
  ];
  for (const filename of new Set(candidates)) {
    if (!filename.startsWith(`${repositoryRoot}${path.sep}`)) continue;
    try {
      if ((await stat(filename)).isFile()) return filename;
    } catch {
      /* TypeScript permits extensionless and .js imports of source .ts files. */
    }
  }
}
async function resolveType(name, info, owner, readModule, seen = new Set()) {
  const key = `${owner}:${name}`;
  if (seen.has(key)) return undefined;
  seen.add(key);
  const declaration = info.types.get(name);
  if (declaration) return { declaration, info, owner };
  const imported = info.imports.get(name);
  if (!imported || imported.imported === '*') return undefined;
  const filename = await resolveImport(owner, imported.source);
  if (!filename) return undefined; // Package types stay named dependencies, not copied definitions.
  const importedInfo = await readModule(filename);
  const local = importedInfo.exports.get(imported.imported) ?? imported.imported;
  return resolveType(local, importedInfo, filename, readModule, seen);
}
function typeTasks(node, info, owner) {
  return [...typeReferences(node)].map((typeName) => ({ typeName, info, owner }));
}
async function dependencyTypes(name, info, owner, readModule) {
  const call = namedCallable(info.exports.get(name) ?? name, info);
  const declarations = new Map();
  if (!call) return declarations;
  const pending = typeTasks(signature(call), info, owner);
  const seen = new Set();
  while (pending.length) {
    const next = pending.pop();
    const key = `${next.owner}:${next.typeName}`;
    if (seen.has(key)) continue;
    seen.add(key);
    const resolved = await resolveType(next.typeName, next.info, next.owner, readModule);
    if (!resolved) continue;
    // Imported aliases are documented under their local name.
    if (!declarations.has(next.typeName)) declarations.set(next.typeName, resolved.declaration);
    pending.push(...typeTasks(resolved.declaration, resolved.info, resolved.owner));
  }
  return declarations;
}
async function checkDocumentedTypes(name, info, owner, apiInfo, context) {
  const declarations = await dependencyTypes(name, info, owner, context.readModule);
  for (const [typeName, apiType] of apiInfo.types) {
    const sourceType = declarations.get(typeName);
    if (!sourceType) {
      context.failures.push(
        `${name}: documented type ${typeName} is not a resolvable API dependency`,
      );
      continue;
    }
    // Local type drift is already reported by compareApi.
    if (info.types.has(typeName)) continue;
    const renamed = { ...sourceType, id: { ...sourceType.id, name: typeName } };
    if (fingerprint(renamed) !== fingerprint(apiType)) {
      context.failures.push(`${name}: imported API type ${typeName} differs from its source owner`);
    }
  }
}
async function checkFile(contract, value, label, failures) {
  if (typeof value !== 'string' || !value.trim() || path.isAbsolute(value)) {
    failures.push(`${contract.name}: invalid repository-relative ${label}`);
    return undefined;
  }
  const filename = path.resolve(repositoryRoot, value);
  if (!filename.startsWith(`${repositoryRoot}${path.sep}`)) {
    failures.push(`${contract.name}: ${label} leaves the repository`);
    return undefined;
  }
  try {
    if (!(await stat(filename)).isFile()) throw new Error('not a file');
  } catch (error) {
    failures.push(`${contract.name}: missing ${label} ${value} (${error.code ?? error.message})`);
    return undefined;
  }
  return filename;
}
async function checkFileLists(contract, failures) {
  for (const kind of ['consumers', 'tests']) {
    if (!Array.isArray(contract[kind])) {
      failures.push(`${contract.name}: missing ${kind} list`);
      continue;
    }
    const seen = new Set();
    for (const value of contract[kind]) {
      if (seen.has(value)) failures.push(`${contract.name}: duplicate ${kind} path ${value}`);
      seen.add(value);
      await checkFile(contract, value, kind, failures);
    }
  }
}
function checkStateStories(contract, { entries, failures }) {
  if (contract.stateStories === undefined) return;
  if (!Array.isArray(contract.stateStories)) {
    failures.push(`${contract.name}: invalid stateStories list`);
    return;
  }
  const seen = new Set();
  for (const story of contract.stateStories) {
    if (typeof story?.label !== 'string' || !story.label.trim())
      failures.push(`${contract.name}: state story ${story?.id} has no label`);
    if (seen.has(story?.id)) failures.push(`${contract.name}: duplicate state story ${story?.id}`);
    seen.add(story?.id);
    if (entries[story?.id]?.type !== 'story')
      failures.push(`${contract.name}: missing state story ${story?.id}`);
  }
}
function playgroundComponentImport(info) {
  let meta = info.bindings.get(info.exports.get('default')) ?? info.anonymousDefault;
  while (meta && ['TSSatisfiesExpression', 'TSAsExpression'].includes(meta.type))
    meta = meta.expression;
  const component = meta?.properties?.find(
    (property) =>
      property.type === 'ObjectProperty' &&
      (property.key.name ?? property.key.value) === 'component',
  );
  return component?.value.type === 'Identifier'
    ? info.imports.get(component.value.name)
    : undefined;
}
function rendersCoreComponent(render, localName) {
  let found = false;
  walk(render, (node) => {
    if (node.type === 'JSXOpeningElement' && node.name?.name === localName) found = true;
  });
  return found;
}
function propertyValue(node, name) {
  return node?.properties?.find((property) => (property.key?.name ?? property.key?.value) === name)
    ?.value;
}
function checkExplicitDefaults(contract, meta, sourceInfo, failures) {
  const fields = propertyValue(meta, 'argTypes')?.properties ?? [];
  for (const field of fields) {
    const summary = propertyValue(
      propertyValue(propertyValue(field.value, 'table'), 'defaultValue'),
      'summary',
    );
    if (summary?.type !== 'StringLiteral') continue;
    const expected = moduleInfo(parseModule(`const value = ${summary.value};`)).bindings.get(
      'value',
    );
    const candidates = [];
    for (const binding of sourceInfo.bindings.values()) {
      const call = callable(binding, sourceInfo);
      if (!call) continue;
      walk(call.fn.params, (node) => {
        if (
          node.type === 'ObjectProperty' &&
          node.key?.name === field.key?.name &&
          node.value.type === 'AssignmentPattern'
        )
          candidates.push(node.value.right);
      });
    }
    if (!candidates.some((value) => fingerprint(value) === fingerprint(expected)))
      failures.push(
        `${contract.name}: explicit Playground default ${field.key.name} differs from Core`,
      );
  }
}
async function checkPlayground(contract, context) {
  if (!contract.playgroundStory) {
    context.failures.push(`${contract.name}: required Playground is missing`);
    return;
  }
  const story = context.entries[contract.playgroundStory];
  if (story?.type !== 'story') {
    context.failures.push(`${contract.name}: missing Playground ${contract.playgroundStory}`);
    return;
  }
  const filename = path.resolve(appRoot, story.importPath);
  const component = playgroundComponentImport(await context.readModule(filename));
  const owner = component ? await resolveImport(filename, component.source) : undefined;
  if (
    component?.imported !== contract.name ||
    owner !== path.resolve(repositoryRoot, contract.source)
  )
    context.failures.push(
      `${contract.name}: Playground metadata must reference its actual Core export`,
    );
  const info = await context.readModule(filename);
  let meta = info.bindings.get(info.exports.get('default')) ?? info.anonymousDefault;
  while (meta && ['TSSatisfiesExpression', 'TSAsExpression'].includes(meta.type))
    meta = meta.expression;
  checkExplicitDefaults(
    contract,
    meta,
    await context.readModule(path.resolve(repositoryRoot, contract.source)),
    context.failures,
  );
  const render = meta?.properties?.find((property) => property.key?.name === 'render');
  const localName = [...info.imports].find(
    ([, value]) => value.imported === contract.name && value.source === component?.source,
  )?.[0];
  if (render && !rendersCoreComponent(render, localName))
    context.failures.push(`${contract.name}: Playground render does not consume its Core export`);
  const docFile = path.join(docsRoot, 'components', `${contract.name}.mdx`);
  const doc = await readFile(docFile, 'utf8');
  const apiImport = doc.match(/import\s+\*\s+as\s+Api\s+from\s+['"]([^'"]+)['"]/u)?.[1];
  const linked = apiImport ? await resolveImport(docFile, apiImport) : undefined;
  if (
    !doc.includes('<Canvas of={Api.Playground}') ||
    !doc.includes('ArgTypes') ||
    linked !== filename
  )
    context.failures.push(`${contract.name}: independent page must connect Playground and API`);
}
function checkContractMetadata(contract, { names, byName, failures }) {
  if (typeof contract.name !== 'string' || !/^[A-Z]\w*$/u.test(contract.name))
    failures.push(`Invalid component contract name: ${contract.name}`);
  if (names.has(contract.name)) failures.push(`${contract.name}: duplicate component contract`);
  names.add(contract.name);
  byName.set(contract.name, contract);
  for (const key of [
    'purpose',
    'use',
    'avoid',
    'anatomy',
    'states',
    'tokens',
    'interaction',
    'api',
  ]) {
    if (typeof contract[key] !== 'string' || !contract[key].trim())
      failures.push(`${contract.name}: missing ${key}`);
  }
}
async function checkContract(contract, context) {
  checkContractMetadata(contract, context);
  const filename = await checkFile(contract, contract.source, 'source', context.failures);
  if (filename && contract.api?.trim()) {
    try {
      const info = await context.readModule(filename);
      const apiInfo = moduleInfo(parseApi(contract.api));
      context.failures.push(...compareApi(contract.name, info, apiInfo));
      await checkDocumentedTypes(contract.name, info, filename, apiInfo, context);
    } catch (error) {
      context.failures.push(`${contract.name}: ${error.message}`);
    }
  }
  checkStateStories(contract, context);
  await checkPlayground(contract, context);
  await checkFileLists(contract, context.failures);
  if (context.entries[contract.storyId]?.type !== 'story')
    context.failures.push(`${contract.name}: missing story ${contract.storyId}`);
  const title = `Design System/Components/${contract.name}`;
  if (
    !Object.values(context.entries).some((entry) => entry.type === 'docs' && entry.title === title)
  )
    context.failures.push(`${contract.name}: missing independent documentation`);
}
function checkPublicOwner(component, filename, { byName, failures }) {
  const relative = path.relative(repositoryRoot, filename).split(path.sep).join('/');
  const contract = byName.get(component.name);
  if (!contract) failures.push(`${relative}: undocumented exported component ${component.name}`);
  else if (path.resolve(repositoryRoot, contract.source) !== filename)
    failures.push(`${component.name}: registry source differs from export owner ${relative}`);
}
async function checkPublicCoverage(context) {
  let count = 0;
  for (const directory of ['src/core/ui', 'src/core/audio-graph']) {
    for (const filename of await sourceFiles(path.join(appRoot, directory))) {
      let info;
      try {
        info = await context.readModule(filename);
      } catch (error) {
        context.failures.push(error.message);
        continue;
      }
      for (const component of exportedComponents(info)) {
        count++;
        checkPublicOwner(component, filename, context);
      }
    }
  }
  return count;
}
async function inspectDocumentLinks(filename, { entries, failures }) {
  const source = await readFile(filename, 'utf8');
  for (const match of source.matchAll(/\?path=\/(story|docs)\/([^"\s)]+?)(?=["\s)])/gu)) {
    const [kind, id] = [match[1], match[2]];
    if (entries[id]?.type !== kind)
      failures.push(`${path.relative(docsRoot, filename)}: unresolved ${kind}/${id}`);
  }
}
async function inspectLinks(directory, context) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const filename = path.join(directory, entry.name);
    if (entry.isDirectory()) await inspectLinks(filename, context);
    else if (entry.name.endsWith('.mdx')) await inspectDocumentLinks(filename, context);
  }
}
async function checkPageOwner(page, context) {
  const filename = path.resolve(repositoryRoot, page.source);
  const info = await context.readModule(filename);
  if (!exportedComponents(info).some((entry) => entry.name === page.component))
    context.failures.push(`${page.title}: missing page export ${page.component}`);
  if (!page.states.length) context.failures.push(`${page.title}: missing page states`);
  for (const id of page.states)
    if (context.entries[id]?.type !== 'story')
      context.failures.push(`${page.title}: missing page state ${id}`);
  return `${filename}:${page.component}`;
}
async function checkScreenCoverage(owners, context) {
  for (const directory of ['src/app', 'src/screens']) {
    for (const filename of await sourceFiles(path.join(appRoot, directory))) {
      const info = await context.readModule(filename);
      for (const entry of exportedComponents(info))
        if (!owners.has(`${filename}:${entry.name}`))
          context.failures.push(`${entry.name}: public App/Screen missing from Pages inventory`);
    }
  }
}
async function checkPageContracts(context) {
  const owners = new Set();
  for (const page of pageContracts) owners.add(await checkPageOwner(page, context));
  await checkScreenCoverage(owners, context);
  const docs = await readFile(path.join(docsRoot, 'Pages.mdx'), 'utf8');
  if (!docs.includes('<PagesReference />'))
    context.failures.push('Pages: missing executable page inventory');
}
async function checkDocumentation() {
  const context = {
    failures: [],
    entries: {},
    names: new Set(),
    byName: new Map(),
    readModule: createModuleReader(),
  };
  try {
    ({ entries: context.entries } = JSON.parse(
      await readFile(path.join(appRoot, 'storybook-static/index.json'), 'utf8'),
    ));
  } catch (error) {
    context.failures.push(`Storybook index unavailable: ${error.message}`);
  }
  for (const contract of componentContracts) await checkContract(contract, context);
  const publicCount = await checkPublicCoverage(context);
  await inspectLinks(docsRoot, context);
  await checkPageContracts(context);
  if (context.failures.length)
    throw new Error(
      `Design system documentation check failed (${context.failures.length}):\n${context.failures.join('\n')}`,
    );
  console.log(
    `Design system documentation checked: ${context.names.size} unique contracts, ${publicCount} public Core UI/audio-graph exports, source API/type/default parity, existing consumer/test paths, independent pages and all fixed links.`,
  );
}

function testPlaygroundMetadata() {
  const info = (source) => moduleInfo(parseModule(source));
  assert.deepEqual(
    playgroundComponentImport(
      info(
        "import {Button as Actual} from './Button'; const meta={component:Actual} satisfies Meta<typeof Actual>; export default meta;",
      ),
    ),
    { source: './Button', imported: 'Button' },
  );
  assert.equal(
    playgroundComponentImport(
      info('function Wrapper(){return <button/>} export default {component:Wrapper}'),
    ),
    undefined,
  );
}
function testPlaygroundRenderAndDefaults() {
  const info = (source) => moduleInfo(parseModule(source));
  const rendered = info('const render = () => <Actual />;').bindings.get('render');
  const fake = info('const render = () => <div>Actual</div>;').bindings.get('render');
  assert.equal(rendersCoreComponent(rendered, 'Actual'), true);
  assert.equal(rendersCoreComponent(fake, 'Actual'), false);
  const source = info('export function Button({ step = 1 }) { return <button />; }');
  const meta = (value) =>
    info(
      `const meta = {argTypes:{step:{table:{defaultValue:{summary:'${value}'}}}}};`,
    ).bindings.get('meta');
  const failures = [];
  checkExplicitDefaults({ name: 'Button' }, meta('1'), source, failures);
  assert.deepEqual(failures, []);
  checkExplicitDefaults({ name: 'Button' }, meta('2'), source, failures);
  assert.match(failures.join('\n'), /default step differs from Core/);
}
function selfTest() {
  testPlaygroundMetadata();
  testPlaygroundRenderAndDefaults();
  const info = (source) => moduleInfo(parseModule(source));
  const source = `type Props = { label: string; tone?: 'neutral' | 'critical' }; export function Button({tone = 'neutral', ...props}: Props) { return <button />; }`;
  const api = `type Props={tone?:'critical'|'neutral'; label:string}; export function Button({tone='neutral',...props}:Props)`;
  assert.deepEqual(compareApi('Button', info(source), moduleInfo(parseApi(api))), []);
  assert.match(
    compareApi(
      'Button',
      info(source),
      moduleInfo(parseApi(api.replace("tone='neutral'", "tone='critical'"))),
    ).join('\n'),
    /signature\/defaults/,
  );
  assert.match(
    compareApi(
      'Button',
      info(source),
      moduleInfo(parseApi(api.replace("'critical'|'neutral'", "'critical'|'neutral'|'warning'"))),
    ).join('\n'),
    /type Props differs/,
  );
  assert.match(
    compareApi(
      'Button',
      info(source),
      moduleInfo(parseApi(api.replace('label:string', 'label?:string'))),
    ).join('\n'),
    /type Props differs/,
  );
  assert.match(
    compareApi(
      'Button',
      info(source),
      moduleInfo(parseApi('export function Button({tone="neutral",...props}:Props)')),
    ).join('\n'),
    /missing source-local type Props/,
  );
  const closure = `interface Base { disabled?: boolean } type Props=Base & { label:string }; export function Button(props:Props){return <button/>}`;
  assert.match(
    compareApi(
      'Button',
      info(closure),
      moduleInfo(parseApi('type Props=Base & {label:string}; export function Button(props:Props)')),
    ).join('\n'),
    /missing source-local type Base/,
  );
  const wrappers = info(
    `import {forwardRef as ref, memo, createContext} from 'react'; const Internal=(props:{label:string})=><span/>; export {Internal as Label}; export const Button=memo(ref<HTMLButtonElement,{label:string}>((props, ref)=><button/>)); export const ButtonContext=createContext(null); export const Palette={red:'red'}; export const useFoo=()=>null;`,
  );
  assert.deepEqual(
    exportedComponents(wrappers)
      .map(({ name }) => name)
      .sort(),
    ['Button', 'Label'],
  );
  assert.deepEqual(
    exportedComponents(info('export default function Button(){return <button/>}')).map(
      ({ name }) => name,
    ),
    ['Button'],
  );
  assert.equal(exportedComponents(info('export function Compute(){return 42}')).length, 0);
  assert.equal(
    exportedComponents(info('export function Descriptor(){return {render:()=> <button/>}}')).length,
    0,
  );
  assert.equal(
    exportedComponents(info('export function FindElement():Element{return document.body}')).length,
    0,
  );
  testDefaultConstants();
  testTypedConstAndGenericScope();
  console.log(
    'Design system API gate self-tests passed: formatting equivalence, default/constant/type drift, recursive local closure, typed const/memo/forwardRef/alias/default exports, generic scopes and context/helper exclusions.',
  );
}
function testDefaultConstants() {
  const info = (source) => moduleInfo(parseModule(source));
  const source = info(
    'const empty = new Map<string,number>(); export function Button({values=empty}:{values?:Map<string,number>}){return <button/>}',
  );
  const api =
    'const empty = new Map<string,number>(); export function Button({values=empty}:{values?:Map<string,number>})';
  assert.deepEqual(compareApi('Button', source, moduleInfo(parseApi(api))), []);
  assert.match(
    compareApi(
      'Button',
      source,
      moduleInfo(parseApi(api.replace('const empty = new Map<string,number>(); ', ''))),
    ).join('\n'),
    /missing source-local default constant empty/,
  );
  assert.match(
    compareApi(
      'Button',
      source,
      moduleInfo(parseApi(api.replace('number>();', 'string>();'))),
    ).join('\n'),
    /default constant empty differs/,
  );
}
function testTypedConstAndGenericScope() {
  const info = (source) => moduleInfo(parseModule(source));
  const source =
    'type Props={label:string}; export const Button:React.FC<Props>=(props)=><button/>';
  const api = source.replace('<button/>', '<span/>');
  assert.deepEqual(compareApi('Button', info(source), info(api)), []); // Implementation bodies are excluded.
  assert.match(
    compareApi('Button', info(source), info(api.replace('React.FC<Props>', 'React.FC<{}>'))).join(
      '\n',
    ),
    /signature\/defaults/,
  );
  assert.deepEqual(
    [...typeReferences(parseModule('type Box<T>={value:T; changed:Outside}').body[0])],
    ['Outside'],
  );
  const wrapper =
    'export const Button=forwardRef<HTMLButtonElement,{label:string}>((props,ref)=><button/>)';
  assert.match(
    compareApi(
      'Button',
      info(wrapper),
      info(wrapper.replace('HTMLButtonElement', 'HTMLAnchorElement')),
    ).join('\n'),
    /signature\/defaults/,
  );
}

if (process.argv.includes('--self-test')) selfTest();
else await checkDocumentation();
