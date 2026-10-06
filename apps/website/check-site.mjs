import assert from 'node:assert/strict';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { dirname, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const directory = dirname(fileURLToPath(import.meta.url));
const pages = ['index.html', 'guide.html'];
const sources = new Map(pages.map((page) => [page, readFileSync(resolve(directory, page), 'utf8')]));
const idsByPage = new Map(pages.map((page) => [page, new Set([...sources.get(page).matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]))]));
const externalReference = /^(?:[a-z][a-z\d+.-]*:|\/\/)/i;

function resolveLocalReference(page, reference) {
  assert.ok(!externalReference.test(reference), `${page}: external destination ${reference} is not supported in this prerelease draft`);
  const [pathname, fragment] = reference.split('#', 2);
  const targetPath = resolve(directory, dirname(page), pathname || page);
  const relativePath = relative(directory, targetPath);
  assert.ok(relativePath && !relativePath.startsWith('..'), `${page}: destination ${reference} must stay inside apps/website`);
  assert.ok(existsSync(targetPath) && statSync(targetPath).isFile(), `${page}: local destination ${reference} exists`);
  return { targetPath, fragment };
}

for (const [page, html] of sources) {
  assert.match(html, /<!doctype html>/i, `${page}: HTML doctype`);
  assert.match(html, /<html\s+lang="ko">/i, `${page}: Korean document language`);
  assert.match(html, /<meta\s+name="viewport"/i, `${page}: responsive viewport`);
  assert.match(html, /<link\s+rel="stylesheet"\s+href="styles\.css"/i, `${page}: local stylesheet`);
  assert.match(html, /<main\b[^>]*\bid="main"/i, `${page}: main landmark`);
  assert.equal((html.match(/<h1\b/gi) ?? []).length, 1, `${page}: exactly one h1`);
  assert.match(html, /<nav\b[^>]*aria-label=/i, `${page}: named navigation landmark`);
  assert.match(html, /사전 공개 초안/, `${page}: prerelease draft disclosure`);

  const ids = [...html.matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]);
  assert.equal(new Set(ids).size, ids.length, `${page}: unique id values`);

  for (const image of html.matchAll(/<img\b([^>]*)>/gi)) {
    assert.match(image[1], /\balt="[^"]*"/i, `${page}: image has alt text`);
  }
  for (const svg of html.matchAll(/<svg\b([^>]*)>([\s\S]*?)<\/svg>/gi)) {
    if (/\brole="img"/i.test(svg[1])) {
      assert.match(svg[1], /aria-labelledby=/i, `${page}: informative SVG has an accessible name`);
      assert.match(svg[2], /<title\b/i, `${page}: informative SVG has a title`);
      assert.match(svg[2], /<desc\b/i, `${page}: informative SVG has a description`);
    }
  }

  for (const anchor of html.matchAll(/<a\b([^>]*)>([\s\S]*?)<\/a>/gi)) {
    const attributes = anchor[1];
    const label = anchor[2].replace(/<[^>]*>/g, '').replace(/&nbsp;/g, ' ').trim();
    assert.ok(label || /\baria-label="[^"]+"/i.test(attributes) || /\btitle="[^"]+"/i.test(attributes), `${page}: link has an accessible name`);
    const href = attributes.match(/\bhref="([^"]+)"/i)?.[1];
    assert.ok(href, `${page}: anchor has an href`);
    const { targetPath, fragment } = resolveLocalReference(page, href);
    if (fragment) {
      const targetPage = relative(directory, targetPath);
      assert.ok(idsByPage.get(targetPage)?.has(fragment), `${page}: fragment ${href} exists in ${targetPage}`);
    }
  }

  for (const reference of html.matchAll(/<(?:link|img|script|source|video|audio)\b([^>]*)>/gi)) {
    const attributes = reference[1];
    const value = attributes.match(/\b(?:href|src)="([^"]+)"/i)?.[1];
    if (value) resolveLocalReference(page, value);
  }
}

const styles = readFileSync(resolve(directory, 'styles.css'), 'utf8');
for (const reference of styles.matchAll(/url\((?:\s*["']?)([^)"']+)(?:["']?\s*)\)/gi)) {
  resolveLocalReference('styles.css', reference[1].trim());
}
assert.ok(!/@import\s+(?:url\()?\s*["']?https?:/i.test(styles), 'styles.css: no remote imports');

const home = sources.get('index.html');
for (const phrase of ['14일', '30일', '다운로드와 구매는 제공하지 않습니다', '구매 스토어는 아직 개설되지 않았습니다']) {
  assert.ok(home.includes(phrase), `index.html: truthful availability and license detail includes “${phrase}”`);
}
const guide = sources.get('guide.html');
for (const phrase of ['그래프와 연결', '프로세서와 제어', '녹음하기', 'Stream Deck', '작업 공간과 종료', '평가판과 라이선스']) {
  assert.ok(guide.includes(phrase), `guide.html: manual topic includes “${phrase}”`);
}

console.log(`Static website checks passed for ${pages.length} pages (structure, local destinations/assets, and basic accessibility).`);
