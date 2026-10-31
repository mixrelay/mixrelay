import assert from 'node:assert/strict';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { basename, dirname, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertPublicDestination, checkPublicDestinations } from '../../scripts/check-public-destinations.mjs';

checkPublicDestinations();

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
  assertPublicDestination(reference.replaceAll('&amp;', '&'), page);
}

const manualLanguages = readdirSync(resolve(directory, 'manual'), { withFileTypes: true }).filter((entry) => entry.isDirectory()).map((entry) => entry.name).sort();
assert.ok(manualLanguages.includes('en') && manualLanguages.includes('ko'), 'English and Korean manual sources exist');
const englishChapters = pages.filter((page) => page.startsWith('manual/en/')).map((page) => basename(page));
const requiredChapters = ['index', 'installation', 'first-launch', 'workspace', 'devices', 'app-capture', 'recording', 'effects', 'plugins', 'stream-deck', 'midi', 'licensing', 'updates', 'uninstall', 'troubleshooting', 'support'].map((slug) => `${slug}.html`);
assert.deepEqual(englishChapters, [...requiredChapters].sort(), 'all requested manual chapters exist');
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
    const chapterNav = html.match(/<nav class="guide-nav"[^>]*>([\s\S]*?)<\/nav>/)?.[1];
    assert.deepEqual([...chapterNav.matchAll(/href="([^"]+)"/g)].map((link) => link[1]), requiredChapters, `${page}: chapter navigation includes every chapter once in reading order`);
    assert.match(chapterNav, new RegExp(`href="${chapter}" aria-current="page"`), `${page}: chapter navigation marks its own page`);
    assert.match(html, /aria-current="page"/, `${page}: current chapter identified`);
    for (const target of manualLanguages) {
      assert.ok(html.includes(`hreflang="${target}" href="../${target}/${chapter}"`), `${page}: alternate metadata for ${target}`);
      assert.ok(html.includes(`href="../${target}/${chapter}" lang="${target}" hreflang="${target}"`), `${page}: switch to same chapter in ${target}`);
    }
  }
  assert.doesNotMatch(html, /14일|평가판\s*·|영구 무료 기능 요금제는 없습니다|기간이 끝난 뒤 계속 사용하려면 구매가 필요/, `${page}: no obsolete trial policy`);
  assert.doesNotMatch(html, /그래프·매핑 저장 및 불러오기는 유료 기능|유료 라이선스로 추가 기능과 저장·복원/, `${page}: no obsolete paid-only saving policy`);
  for (const price of html.matchAll(/US\$\d+(?:\.\d+)?/g)) {
    assert.ok(['US$19', 'US$29'].includes(price[0]), `${page}: only confirmed early-adopter and 1.0 prices`);
  }
}

const styles = readFileSync(resolve(directory, 'styles.css'), 'utf8');
for (const reference of styles.matchAll(/url\((?:\s*["']?)([^)"']+)(?:["']?\s*)\)/gi)) resolveLocalReference('styles.css', reference[1].trim());
assert.ok(!/@import\s+(?:url\()?\s*["']?https?:/i.test(styles), 'no remote stylesheet imports');
const home = sources.get('index.html');
const features = [...home.matchAll(/<article class="feature-card">([\s\S]*?)<\/article>/g)].map((entry) => entry[1]);
assert.equal(features.length, 5, 'home: five product features');
assert.match(features[3], /STREAM DECK/, 'home: Stream Deck precedes MIDI');
assert.match(features[4], /MIDI/, 'home: MIDI feature follows Stream Deck');
assert.match(features[4], /href="manual\/ko\/midi.html"/, 'home: MIDI feature opens its manual');
assert.match(home, /id="business-info-title"/, 'home: business-information block exists');
for (const detail of ['에코 비전 랩', '111-42-04359', '505-S80', '+82 10-6380-9646', 'support@rawsector.com', '배승현']) {
  assert.ok(home.includes(detail), `home: business information includes ${detail}`);
}
const legalSections = {
  terms: ['purchase', 'license', 'refunds', 'free', 'support', 'business'],
  privacy: ['scope', 'purchases', 'processing', 'providers', 'rights', 'contact', 'changes'],
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
    if (name === 'LICENSE') {
      const purchases = readFileSync(copy, 'utf8').split(/^2\. /m)[1]?.split(/^3\. /m)[0];
      for (const version of ['1.0', '0.x', '1.x']) assert.ok(purchases?.includes(version), `${language}: agreement update rights include ${version}`);
      assert.doesNotMatch(purchases, /US\$\d+/, `${language}: agreement does not state prices`);
    }
    assert.ok(sources.get(`legal/${language}/terms.html`).includes(`href="${name}.txt"`), `${language}: terms link to ${name}`);
  }
}
for (const phrase of ['기간 제한 없이', '무료 Driver', '최대 64채널', '다운로드와 구매는 제공하지 않습니다', '구매 스토어는 아직 개설되지 않았습니다']) assert.ok(home.includes(phrase), `home: confirmed availability includes ${phrase}`);
assert.ok(sources.get('guide.html').includes('manual/ko/index.html') && sources.get('guide.html').includes('manual/en/index.html'), 'legacy guide links to both manuals');
for (const [language, phrases] of [['en', ['30 days', 'unlimited number', '30 calendar days', 'Deactivate this device']], ['ko', ['30일', '수 제한 없이', '30달력일', '이 기기 비활성화']]]) {
  const license = sources.get(`manual/${language}/licensing.html`);
  for (const phrase of phrases) assert.ok(license.includes(phrase), `${language}: confirmed license policy includes ${phrase}`);
}
const pricingPages = ['index.html', ...['en', 'ko'].map((language) => `legal/${language}/terms.html`),
  ...manualLanguages.map((language) => `manual/${language}/licensing.html`)];
for (const page of pricingPages) {
  const html = sources.get(page);
  assert.deepEqual([...html.matchAll(/US\$\d+/g)].map((match) => match[0]), ['US$19', 'US$29'], `${page}: early-adopter price followed by the future 1.0 price`);
  for (const version of ['1.0', '0.x', '1.x', '2.0']) assert.ok(html.includes(version), `${page}: pricing and update rights include ${version}`);
  assert.doesNotMatch(html, /<(?:del|s|strike)\b|line-through|\d+\s*%/i, `${page}: plain pricing without strikethrough or percentage discounts`);
}
const translatedPricing = {
  de: ['Frühkäuferpreis', 'vor der Veröffentlichung von MixRelay 1.0', 'Ab der Veröffentlichung von 1.0', 'jeweils ohne Steuern', 'Vor der Veröffentlichung von 1.0 gekaufte Lizenzen', 'bleibt optional'],
  en: ['early-adopter price', 'before MixRelay 1.0 is released', 'From the 1.0 release', 'Both prices exclude tax', 'Purchases made before 1.0 include updates for all', 'is optional'],
  es: ['precio para los primeros usuarios', 'antes del lanzamiento de MixRelay 1.0', 'A partir del lanzamiento de la versión 1.0', 'Ambos precios excluyen impuestos', 'Las compras anteriores al lanzamiento de la versión 1.0 incluyen', 'es opcional'],
  fr: ['tarif pour les premiers utilisateurs', 'avant la sortie de MixRelay 1.0', 'À compter de la sortie de la version 1.0', 'Ces deux tarifs sont hors taxes', 'Les achats effectués avant la sortie de la version 1.0 incluent', 'reste facultatif'],
  it: ['prezzo per i primi utenti', 'prima del rilascio di MixRelay 1.0', 'Dal rilascio della versione 1.0', 'Entrambi i prezzi sono al netto delle imposte', 'Gli acquisti effettuati prima del rilascio della versione 1.0 includono', 'resta facoltativo'],
  ja: ['早期購入価格', 'MixRelay 1.0のリリース前', '1.0のリリース日から', 'いずれも税別', '1.0のリリース前に購入した場合、すべての', '購入するかは任意'],
  ko: ['얼리 어답터 가격', 'MixRelay 1.0 출시 전', '1.0 출시일부터', '두 가격 모두 세금 별도', '1.0 출시 전 구매에는 모든', '구매는 선택'],
  pt: ['preço para os primeiros usuários', 'antes do lançamento do MixRelay 1.0', 'A partir do lançamento da versão 1.0', 'Os dois preços não incluem impostos', 'As compras feitas antes do lançamento da versão 1.0 incluem', 'continua sendo opcional'],
  ru: ['Цена для первых покупателей', 'до выхода MixRelay 1.0', 'Со дня выхода версии 1.0', 'Обе цены указаны без налогов', 'Покупки до выхода версии 1.0 включают', 'остаётся необязательной'],
  'zh-Hans': ['早期用户价格', 'MixRelay 1.0 发布前购买', '1.0 发布之日起价格', '均不含税', '在 1.0 发布前购买，包含所有', '后续主要版本由您决定'],
  'zh-Hant': ['早期使用者價格', 'MixRelay 1.0 發布前購買', '1.0 發布之日起價格', '均未含稅', '在 1.0 發布前購買，包含所有', '後續主要版本由您決定'],
};
assert.deepEqual(Object.keys(translatedPricing).sort(), manualLanguages, 'pricing policy covers every manual language');
for (const [language, phrases] of Object.entries(translatedPricing)) {
  for (const page of [`manual/${language}/licensing.html`, ...(language === 'en' || language === 'ko' ? [`legal/${language}/terms.html`] : []), ...(language === 'ko' ? ['index.html'] : [])]) {
    for (const phrase of phrases) assert.ok(sources.get(page).includes(phrase), `${page}: confirmed pricing policy includes ${phrase}`);
  }
}
for (const language of manualLanguages) {
  const page = `manual/${language}/stream-deck.html`;
  const html = sources.get(page);
  assert.match(html, /macOS 14\.2/, `${page}: supported macOS minimum`);
  assert.match(html, /Stream Deck 7\.1/, `${page}: SDK 3.x host minimum`);
  assert.doesNotMatch(html, /Stream Deck 6\.6/, `${page}: no obsolete host requirement`);
  assert.doesNotMatch(html, /\.streamDeckPlugin|href="https:\/\/rawsector\.com\/products\/mixrelay"/, `${page}: no direct plug-in download`);
  assert.match(html, /Elgato Marketplace/, `${page}: Marketplace-only installation`);
}

for (const [page, source] of sources) {
  if (!/^manual\/[^/]+\/midi\.html$/.test(page)) continue;
  for (const behaviour of ['Command+M', 'Ctrl+M', 'Escape', 'Min/Max', 'Delete/Backspace', 'Command+Z', 'Ctrl+Z']) {
    assert.ok(source.includes(behaviour), `${page}: documents MIDI map mode and identity recovery (${behaviour})`);
  }
}

console.log(`Static website checks passed for ${pages.length} pages (links/assets, languages, headings, translation parity, accessibility structure and prerelease policy).`);
