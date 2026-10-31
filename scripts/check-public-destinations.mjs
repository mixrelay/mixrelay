/**
 * Public destination contract. Scope: README/CHANGELOG; shipped licence/legal texts
 * (including bundled third-party notices); control-ui production TS/TSX/CSS and
 * locales; desktop/native and licensing production sources (menus/dialogs included); installer
 * resources and generators; Linux recipes/desktop entries; Cargo/npm/Tauri package
 * metadata; Stream Deck manifest, runtime and property inspector; website HTML,
 * CSS, JS, JSON and legal text; checked-in update/release manifests and generated
 * release manifests before writing. Arch metadata is inspected after substitution.
 * Excludes tests/fixtures (including inline Rust test modules), Storybook/stories,
 * developer docs, build output, dependency source, CI/qualification evidence,
 * font provenance, loopback development/Stream Deck transports, XML namespaces/DTDs
 * and JSON schema identifiers.
 * This is an offline allowlist, not a network availability or mailbox delivery test.
 */
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { renderArchRecipe } from './prepare-arch-driver-package.mjs';

const root = fileURLToPath(new URL('..', import.meta.url));
const publicHosts = new Set([
  'rawsector.com', 'appserver.rawsector.com', 'downloads.rawsector.com',
  'mixrelay.lemonsqueezy.com', 'www.lemonsqueezy.com', 'api.lemonsqueezy.com',
  'support.apple.com', 'law.go.kr', 'www.law.go.kr', 'www.easylaw.go.kr',
]);
const publicEmails = new Set([
  'support@rawsector.com', 'privacy@dsub.io', 'legal@dsub.io', 'giovanni@fragcolor.com',
]);
// Existing third-party licence references retain their original protocols.
const licenceDestinations = new Set([
  'http://scripts.sil.org/OFL', 'https://openfontlicense.org',
  'http://www.apache.org/licenses/LICENSE-2.0', 'http://www.apache.org/licenses',
  'https://github.com/alsa-project/alsa-lib',
]);

export function assertPublicDestination(destination, surface = 'public surface') {
  const fail = () => { throw new Error(`${surface}: unapproved public destination: ${destination}`); };
  if (destination.startsWith('mailto:') || !destination.includes('://') && !destination.startsWith('//')) {
    const email = destination.replace(/^mailto:/, '').split('?', 1)[0].toLowerCase();
    if (!publicEmails.has(email)) fail();
    return;
  }
  let url;
  try { url = new URL(destination.startsWith('//') ? `https:${destination}` : destination); }
  catch { fail(); }
  if (url.username || url.password || url.port) fail();
  if (licenceDestinations.has(destination.replace(/\/$/, ''))) return;
  if (url.protocol !== 'https:' || !publicHosts.has(url.hostname)) fail();
  if (/\.streamDeckPlugin(?:$|[?#])/i.test(url.pathname)) fail();
}

export function extractDestinations(source) {
  const text = source.replaceAll('&amp;', '&').replaceAll('&#64;', '@')
    .replaceAll('&#58;', ':').replaceAll('&#47;', '/');
  const urls = [...text.matchAll(/(?:[a-z][a-z\d+.-]*:\/\/|\/\/)[a-z\d][^\s<>"'`\\{}\u0080-\uffff]*/gi)]
    .map(match => match[0].replace(/[),.;\]]+$/, ''));
  const emails = [...text.matchAll(/(?<![\w/])[a-z\d._%+-]+@[a-z\d](?:[a-z\d.-]*[a-z\d])?\.[a-z]{2,}/gi)]
    .map(match => match[0]);
  return [...new Set([...urls, ...emails])];
}

export function assertPublicReferences(source, surface) {
  for (const destination of extractDestinations(source)) assertPublicDestination(destination, surface);
  if ((surface === 'README.md' || /^apps\/website\/manual\//.test(surface)) && /\.streamDeckPlugin/i.test(source)) {
    throw new Error(`${surface}: customer plug-in installation must use Marketplace`);
  }
  if (/\b(?:github\.com\/mixrelay\/|mixrelay\.com\b|Unknown Packager\b)/i.test(source)) {
    throw new Error(`${surface}: private, unresolved or anonymous public reference`);
  }
}

export function isUserFacingSurface(path) {
  if (/(?:^|\/)(?:tests?|test-support|fixtures?|__tests__|node_modules|target|dist|storybook)(?:\/|[._-])|\.(?:test|stories|spec)\.|(?:^|_)tests?\.rs$/.test(path)) return false;
  if (['README.md', 'CHANGELOG.md', 'Cargo.toml'].includes(path) || /^LICENSE.*\.txt$/.test(path)) return true;
  if (/^(?:apps\/[^/]+\/)?package\.json$/.test(path)) return true;
  if (/^apps\/website\//.test(path)) return !/\/fonts\/sources\.json$/.test(path) && /\.(?:html|css|js|json|txt)$/.test(path) && !/check-site\.mjs$/.test(path);
  if (/^apps\/control-ui\/src\//.test(path)) return /\.(?:ts|tsx|css|txt)$/.test(path);
  if (/^crates\/licensing\/src\//.test(path)) return /\.rs$/.test(path);
  if (/^apps\/desktop\/src-tauri\//.test(path)) return /(?:src\/.*\.(?:rs|m)|Cargo\.toml|tauri[^/]*\.json|Info\.plist)$/.test(path);
  if (/^native\//.test(path)) return /\.(?:rs|m|mm|c|h|plist|strings)$/.test(path);
  if (/^packaging\//.test(path)) return !/\.md$/.test(path);
  if (/^apps\/streamdeck-plugin\//.test(path)) return /\.(?:json|html|mjs|css)$/.test(path);
  if (/^(?:third-party\/|vendor\/LICENSE)/.test(path)) return /(?:LICENSE[^/]*|COPYING|NOTICE)$/.test(path);
  if (/^scripts\/(?:build-macos-installer-pkgs|build-macos-removal-app|prepare-macos-release-payload|prepare-release-manifest)\.mjs$/.test(path)) return true;
  return /(?:^|\/)[^/]+\.(?:release|update)\.json$/.test(path) && !/^(?:docs|\.github|\.lab)\//.test(path);
}

export function publicSurfaceText(source, path) {
  let text = source.replace(/<!DOCTYPE[^>]*>/g, '').replace(/\bxmlns(?::\w+)?="[^"]*"/g, '')
    .replace(/"\$schema"\s*:\s*"[^"]*"/g, '');
  if (/\.(?:rs|m|mm|c|h|ts|tsx|mjs|js|css)$/.test(path)) {
    text = text.replace(/^#\[cfg\(test\)\]\s*\nmod tests \{[\s\S]*?^}/gm, '')
      .replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '');
  }
  // Development transport addresses are not customer destinations.
  if (/tauri[^/]*\.json$/.test(path)) text = text.replace(/"devUrl"\s*:\s*"[^"]*"/g, '');
  return text.replaceAll('ws://127.0.0.1:*', '');
}

export function checkPublicDestinations(directory = root) {
  const files = execFileSync('git', ['ls-files', '--cached', '--others', '--exclude-standard', '-z'], { cwd: directory, encoding: 'utf8' })
    .split('\0').filter(Boolean).filter(isUserFacingSurface);
  for (const path of files) assertPublicReferences(publicSurfaceText(readFileSync(resolve(directory, path), 'utf8'), path), path);
  assertPublicReferences(JSON.stringify(JSON.parse(readFileSync(resolve(directory, 'support/environments.json'))).metadata), 'support product metadata');
  assertPublicReferences(renderArchRecipe({ version: '0.1.0', architecture: 'amd64', checksum: 'a'.repeat(64) }), 'rendered Arch metadata');
  return files.length;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try { console.log(`Public destinations passed (${checkPublicDestinations()} surfaces).`); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
