import assert from 'node:assert/strict';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { basename, dirname, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const directory = dirname(fileURLToPath(import.meta.url));
function htmlPages(folder) {
  return readdirSync(folder, { withFileTypes: true }).flatMap((entry) => {
    const path = resolve(folder, entry.name);
    return entry.isDirectory() ? htmlPages(path) : entry.name.endsWith('.html') ? [relative(directory, path)] : [];
  }).sort();
}
const pages = htmlPages(directory);
const sources = new Map(pages.map((page) => [page, readFileSync(resolve(directory, page), 'utf8').replace(/<!--[\s\S]*?-->/g, '')]));
const idsByPage = new Map(pages.map((page) => [page, new Set([...sources.get(page).matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]))]));
const externalReference = /^(?:[a-z][a-z\d+.-]*:|\/\/)/i;
const legalReferences = new Set([
  'mailto:privacy@dsub.io',
  'https://www.lemonsqueezy.com/buyer-terms',
  'https://www.lemonsqueezy.com/privacy',
  'https://rawsector.com/privacy',
  'https://www.law.go.kr/LSW/lsLinkCommonInfo.do?chrClsCd=010202&lsJoLnkSeq=1031805939',
  'https://www.law.go.kr/LSW/lsLinkCommonInfo.do?chrClsCd=010202&lsJoLnkSeq=1031805883',
  'https://law.go.kr/lsLinkCommonInfo.do?lsJoLnkSeq=1020398611',
  'https://law.go.kr/lsLinkCommonInfo.do?chrClsCd=010202&lsJoLnkSeq=1029332499',
  'https://www.law.go.kr/LSW/lsLinkCommonInfo.do?lsJoLnkSeq=1033215161',
  'https://www.easylaw.go.kr/CSP/CnpClsMain.laf?ccfNo=2&cciNo=3&cnpClsNo=2&csmSeq=2985&popMenu=ov',
]);

function resolveLocalReference(page, reference) {
  assert.ok(!externalReference.test(reference), `${page}: expected local asset ${reference}`);
  const [pathname, fragment] = reference.split('#', 2);
  const targetPath = resolve(directory, dirname(page), decodeURIComponent(pathname || basename(page)));
  const relativePath = relative(directory, targetPath);
  assert.ok(relativePath && !relativePath.startsWith('..'), `${page}: destination ${reference} must stay inside apps/website`);
  assert.ok(existsSync(targetPath) && statSync(targetPath).isFile(), `${page}: local destination ${reference} exists`);
  if (fragment) {
    assert.ok(idsByPage.get(relativePath)?.has(decodeURIComponent(fragment)), `${page}: fragment ${reference} exists in ${relativePath}`);
  }
  return relativePath;
}
function checkAnchor(page, reference) {
  if (!externalReference.test(reference)) return resolveLocalReference(page, reference);
  // The prerelease site has no public store or download destination yet.
  assert.ok(reference === 'mailto:support@mixrelay.com' ||
    (page.startsWith('legal/') && legalReferences.has(reference.replaceAll('&amp;', '&'))) ||
    (reference.startsWith('https://support.apple.com/') && new URL(reference).hostname === 'support.apple.com'),
  `${page}: external destination is an approved support or legal reference: ${reference}`);
}

const manualLanguages = readdirSync(resolve(directory, 'manual'), { withFileTypes: true }).filter((entry) => entry.isDirectory()).map((entry) => entry.name).sort();
assert.ok(manualLanguages.includes('en') && manualLanguages.includes('ko'), 'English and Korean manual sources exist');
const englishChapters = pages.filter((page) => page.startsWith('manual/en/')).map((page) => basename(page));
const requiredChapters = ['index', 'installation', 'first-launch', 'workspace', 'devices', 'app-capture', 'recording', 'effects', 'plugins', 'stream-deck', 'licensing', 'updates', 'uninstall', 'troubleshooting', 'support'].map((slug) => `${slug}.html`).sort();
assert.deepEqual(englishChapters, requiredChapters, 'all requested manual chapters exist');
for (const language of manualLanguages) {
  assert.deepEqual(pages.filter((page) => page.startsWith(`manual/${language}/`)).map((page) => basename(page)), englishChapters, `${language}: translated chapter paths match English`);
}

for (const [page, html] of sources) {
  const language = page.startsWith('legal/') ? page.split('/')[1] :
    page.startsWith('manual/') ? (page.split('/').length === 3 ? page.split('/')[1] : 'en') : 'ko';
  assert.match(html, /<!doctype html>/i, `${page}: HTML doctype`);
  assert.ok(html.includes(`<html lang="${language}">`), `${page}: correct document language`);
  assert.match(html, /<meta\s+name="viewport"/i, `${page}: responsive viewport`);
  assert.match(html, /<meta\s+name="description"\s+content="[^"]+"/i, `${page}: page description`);
  assert.match(html, /<title>[^<]+<\/title>/i, `${page}: page title`);
  assert.match(html, /<link\s+rel="stylesheet"\s+href="(?:\.\.\/)*styles\.css"/i, `${page}: shared local stylesheet`);
  assert.equal((html.match(/<main\b/gi) ?? []).length, 1, `${page}: exactly one main landmark`);
  assert.match(html, /<main\b[^>]*\bid="main"/i, `${page}: main target`);
  assert.match(html, /class="skip-link" href="#main"/, `${page}: skip link`);
  assert.equal((html.match(/<h1\b/gi) ?? []).length, 1, `${page}: exactly one h1`);
  assert.match(html, /<nav\b[^>]*aria-label=/i, `${page}: named navigation landmark`);
  const policyLanguage = language === 'ko' ? 'ko' : 'en';
  const footer = html.match(/<footer\b[^>]*class="site-footer"[^>]*>([\s\S]*?)<\/footer>/i)?.[1];
  assert.ok(footer, `${page}: site footer exists`);
  const footerDestinations = [...footer.matchAll(/\bhref="([^"]+)"/g)]
    .map((match) => match[1]).filter((reference) => !externalReference.test(reference))
    .map((reference) => resolveLocalReference(page, reference));
  for (const policy of ['terms', 'privacy']) {
    const expected = `legal/${policyLanguage}/${policy}.html`;
    assert.ok(footerDestinations.includes(expected), `${page}: footer links to ${expected}`);
  }
  // Every language carries the same marked notice; its wording is translated.
  assert.ok(/class="draft-badge"/.test(html) || html.includes(language === 'ko' ? '사전 공개 초안' : 'Prerelease draft'), `${page}: prerelease disclosure`);
  let previousLevel = 0;
  for (const heading of html.matchAll(/<h([1-6])\b[^>]*>([\s\S]*?)<\/h\1>/gi)) {
    const level = Number(heading[1]);
    assert.ok(level <= previousLevel + 1, `${page}: heading levels do not skip from h${previousLevel} to h${level}`);
    assert.ok(heading[2].replace(/<[^>]*>/g, '').trim(), `${page}: nonempty heading`);
    previousLevel = level;
  }
  const ids = [...html.matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]);
  assert.equal(new Set(ids).size, ids.length, `${page}: unique ids`);
  for (const name of html.matchAll(/\baria-labelledby="([^"]+)"/g)) {
    for (const id of name[1].split(/\s+/)) assert.ok(idsByPage.get(page).has(id), `${page}: accessible label ${id} exists`);
  }
  for (const image of html.matchAll(/<img\b([^>]*)>/gi)) assert.match(image[1], /\balt="[^"]*"/i, `${page}: image alt text`);
  for (const svg of html.matchAll(/<svg\b([^>]*)>([\s\S]*?)<\/svg>/gi)) {
    if (/\brole="img"/i.test(svg[1])) {
      assert.match(svg[1], /aria-labelledby=/i, `${page}: informative SVG named`);
      assert.match(svg[2], /<title\b/i, `${page}: SVG title`);
      assert.match(svg[2], /<desc\b/i, `${page}: SVG description`);
    }
  }
  for (const anchor of html.matchAll(/<a\b([^>]*)>([\s\S]*?)<\/a>/gi)) {
    const attributes = anchor[1];
    const label = anchor[2].replace(/<[^>]*>/g, '').replace(/&nbsp;/g, ' ').trim();
    assert.ok(label || /\baria-label="[^"]+"/i.test(attributes) || /\btitle="[^"]+"/i.test(attributes), `${page}: named link`);
    const href = attributes.match(/\bhref="([^"]+)"/i)?.[1];
    assert.ok(href, `${page}: anchor href`);
    checkAnchor(page, href);
    if (/\bhreflang=/.test(attributes)) assert.match(attributes, /\blang="[^"]+"/, `${page}: language link declares its text language`);
  }
  for (const reference of html.matchAll(/<(?:link|img|script|source|video|audio)\b([^>]*)>/gi)) {
    const value = reference[1].match(/\b(?:href|src)="([^"]+)"/i)?.[1];
    if (value) resolveLocalReference(page, value);
  }
  if (page.split('/').length === 3 && page.startsWith('manual/')) {
    const chapter = basename(page);
    const englishIds = [...idsByPage.get(`manual/en/${chapter}`)].sort();
    assert.deepEqual([...idsByPage.get(page)].sort(), englishIds, `${page}: section IDs match English`);
    assert.match(html, /class="guide-nav"/, `${page}: chapter navigation`);
    assert.match(html, /aria-current="page"/, `${page}: current chapter identified`);
    for (const target of manualLanguages) {
      assert.ok(html.includes(`hreflang="${target}" href="../${target}/${chapter}"`), `${page}: alternate metadata for ${target}`);
      assert.ok(html.includes(`href="../${target}/${chapter}" lang="${target}" hreflang="${target}"`), `${page}: switch to same chapter in ${target}`);
    }
  }
  assert.doesNotMatch(html, /14일|평가판\s*·|영구 무료 기능 요금제는 없습니다|기간이 끝난 뒤 계속 사용하려면 구매가 필요/, `${page}: no obsolete trial policy`);
  assert.doesNotMatch(html, /그래프·매핑 저장 및 불러오기는 유료 기능|유료 라이선스로 추가 기능과 저장·복원/, `${page}: no obsolete paid-only saving policy`);
}

const styles = readFileSync(resolve(directory, 'styles.css'), 'utf8');
for (const reference of styles.matchAll(/url\((?:\s*["']?)([^)"']+)(?:["']?\s*)\)/gi)) resolveLocalReference('styles.css', reference[1].trim());
assert.ok(!/@import\s+(?:url\()?\s*["']?https?:/i.test(styles), 'no remote stylesheet imports');
const home = sources.get('index.html');
assert.match(home, /id="business-info-title"/, 'home: business-information block exists');
for (const detail of ['에코비전랩', '111-42-04359', '505-S80', '+82 10-6380-9646', 'support@mixrelay.com', '배승현', '[통신판매업 신고번호 또는 면제 사유 확인 필요]']) {
  assert.ok(home.includes(detail), `home: business information includes ${detail}`);
}
const legalSections = {
  terms: ['refunds', 'purchase', 'license', 'free', 'activation', 'withdrawal', 'support', 'agreements', 'business'],
  privacy: ['scope', 'processing', 'records', 'destruction', 'providers', 'overseas', 'automatic', 'rights', 'security', 'contact', 'changes'],
};
assert.deepEqual(pages.filter((page) => page.startsWith('legal/')), ['en', 'ko'].flatMap((language) =>
  ['privacy', 'terms'].map((policy) => `legal/${language}/${policy}.html`)), 'all four legal pages exist');
for (const language of ['en', 'ko']) {
  for (const [policy, sections] of Object.entries(legalSections)) {
    const page = `legal/${language}/${policy}.html`;
    const html = sources.get(page);
    for (const id of sections) assert.ok(idsByPage.get(page).has(id), `${page}: ${id} section exists`);
    assert.deepEqual([...idsByPage.get(page)].sort(), [...idsByPage.get(`legal/en/${policy}.html`)].sort(), `${page}: policy section parity`);
    for (const placeholder of html.matchAll(/\[[^<>\n]+\]/g)) {
      assert.ok(html.includes(`<mark>${placeholder[0]}</mark>`), `${page}: unconfirmed fact is visibly marked: ${placeholder[0]}`);
    }
    for (const target of ['en', 'ko']) {
      assert.ok(html.includes(`hreflang="${target}" href="../${target}/${policy}.html"`), `${page}: alternate metadata for ${target}`);
      assert.ok(html.includes(`href="../${target}/${policy}.html" lang="${target}" hreflang="${target}"`), `${page}: same-policy language switch`);
    }
  }
  for (const name of ['LICENSE', 'LICENSE-Driver']) {
    const original = resolve(directory, '../..', `${name}${language === 'ko' ? '.ko' : ''}.txt`);
    const copy = resolve(directory, 'legal', language, `${name}.txt`);
    assert.equal(readFileSync(copy, 'utf8'), readFileSync(original, 'utf8'), `${language}: website ${name} copy matches the installed agreement`);
    assert.ok(sources.get(`legal/${language}/terms.html`).includes(`href="${name}.txt"`), `${language}: terms link to ${name}`);
  }
}
for (const phrase of ['기간 제한 없이', '무료 Driver', '최대 64채널', '다운로드와 구매는 제공하지 않습니다', '구매 스토어는 아직 개설되지 않았습니다', 'US$69', 'US$39']) assert.ok(home.includes(phrase), `home: confirmed availability and pricing includes ${phrase}`);
assert.ok(sources.get('guide.html').includes('manual/ko/index.html') && sources.get('guide.html').includes('manual/en/index.html'), 'legacy guide links to both manuals');
for (const [language, phrases] of [['en', ['30 days', 'US$69', 'US$39', 'unlimited number', '30 calendar days', 'Deactivate this device']], ['ko', ['30일', 'US$69', 'US$39', '수 제한 없이', '30달력일', '이 기기 비활성화']]]) {
  const license = sources.get(`manual/${language}/licensing.html`);
  for (const phrase of phrases) assert.ok(license.includes(phrase), `${language}: confirmed license policy includes ${phrase}`);
}
console.log(`Static website checks passed for ${pages.length} pages (links/assets, languages, headings, translation parity, accessibility structure and prerelease policy).`);
