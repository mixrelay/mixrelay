import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import test from "node:test";

const languages = JSON.parse(readFileSync(new URL("../packaging/localization/languages.json", import.meta.url), "utf8"));
const removal = JSON.parse(readFileSync(new URL("../packaging/localization/linux-removal.json", import.meta.url), "utf8"));

test("every App contract states the approved 30-day offline authorization separately from refunds", () => {
  for (const entry of languages) {
    const terms = readFileSync(new URL(`../${entry.appTerms}`, import.meta.url), "utf8");
    const activation = terms.split(/^3\. /m)[1]?.split(/^4\. /m)[0];
    assert.ok(activation, `${entry.appTerms}: activation section`);
    assert.match(activation, /30/, `${entry.appTerms}: approved offline authorization period`);
  }
});

test("every App contract states update rights before and from the 1.0 release and retains covered versions", () => {
  const updateRights = {
    en: [
      "A paid licence purchased before version 1.0 is released covers all 0.x and 1.x versions and the updates made available for them.",
      "A paid licence purchased from the 1.0 release covers the purchased major version and the updates made available for it.",
      "If a later major version is offered, purchasing it is optional.",
      "Not purchasing a later major version does not cancel your right to use the versions already covered by your licence.",
    ],
    ko: [
      "1.0 출시 전에 구매한 유료 사용권은 모든 0.x 및 1.x 버전과 해당 버전에 제공되는 업데이트에 적용됩니다.",
      "1.0 출시일부터 구매한 유료 사용권은 구매한 메이저 버전과 해당 버전에 제공되는 업데이트에 적용됩니다.",
      "차기 메이저 버전을 제공하는 경우 그 구매는 선택 사항입니다.",
      "차기 메이저 버전을 구매하지 않아도 사용권에 이미 포함된 버전을 사용할 권리는 유지됩니다.",
    ],
    ja: [
      "1.0のリリース前に購入した有料ライセンスは、すべての0.xおよび1.xバージョンと、それらのバージョン向けに提供される更新を対象とします。",
      "1.0のリリース日以降に購入した有料ライセンスは、購入したメジャーバージョンと、そのバージョン向けに提供される更新を対象とします。",
      "後のメジャーバージョンが提供される場合、その購入は任意です。",
      "後のメジャーバージョンを購入しなくても、ライセンスの対象にすでに含まれるバージョンの利用権は維持されます。",
    ],
    "zh-Hans": [
      "在 1.0 发布前购买的付费许可涵盖所有 0.x 和 1.x 版本及为这些版本提供的更新。",
      "自 1.0 发布之日起购买的付费许可涵盖所购买的主要版本及为该版本提供的更新。",
      "若提供后续主要版本，是否购买由您决定。",
      "不购买后续主要版本不会取消您使用许可已涵盖版本的权利。",
    ],
    "zh-Hant": [
      "在 1.0 發布前購買的付費授權涵蓋所有 0.x 和 1.x 版本及為這些版本提供的更新。",
      "自 1.0 發布之日起購買的付費授權涵蓋所購買的主要版本及為該版本提供的更新。",
      "若提供後續主要版本，是否購買由您決定。",
      "不購買後續主要版本不會取消您使用授權已涵蓋版本的權利。",
    ],
    fr: [
      "Une licence payante achetée avant la sortie de la version 1.0 couvre toutes les versions 0.x et 1.x ainsi que les mises à jour proposées pour ces versions.",
      "Une licence payante achetée à compter de la sortie de la version 1.0 couvre la version majeure achetée ainsi que les mises à jour proposées pour celle-ci.",
      "Si une version majeure ultérieure est proposée, son achat est facultatif.",
      "Ne pas acheter une version majeure ultérieure ne supprime pas votre droit d’utiliser les versions déjà couvertes par votre licence.",
    ],
    de: [
      "Eine vor der Veröffentlichung von Version 1.0 erworbene kostenpflichtige Lizenz gilt für alle Versionen 0.x und 1.x sowie die dafür bereitgestellten Updates.",
      "Eine ab der Veröffentlichung von Version 1.0 erworbene kostenpflichtige Lizenz gilt für die erworbene Hauptversion sowie die dafür bereitgestellten Updates.",
      "Wird eine spätere Hauptversion angeboten, ist ihr Kauf freiwillig.",
      "Der Verzicht auf den Kauf einer späteren Hauptversion hebt Ihr Recht zur Nutzung der bereits von Ihrer Lizenz abgedeckten Versionen nicht auf.",
    ],
    es: [
      "Una licencia de pago comprada antes del lanzamiento de la versión 1.0 cubre todas las versiones 0.x y 1.x y las actualizaciones ofrecidas para ellas.",
      "Una licencia de pago comprada a partir del lanzamiento de la versión 1.0 cubre la versión principal comprada y las actualizaciones ofrecidas para ella.",
      "Si se ofrece una versión principal posterior, comprarla es opcional.",
      "No comprar una versión principal posterior no cancela su derecho a utilizar las versiones ya cubiertas por su licencia.",
    ],
    pt: [
      "Uma licença paga comprada antes do lançamento da versão 1.0 cobre todas as versões 0.x e 1.x e as atualizações oferecidas para elas.",
      "Uma licença paga comprada a partir do lançamento da versão 1.0 cobre a versão principal comprada e as atualizações oferecidas para ela.",
      "Se uma versão principal posterior for oferecida, sua compra será opcional.",
      "Não comprar uma versão principal posterior não cancela o seu direito de utilizar as versões já cobertas pela sua licença.",
    ],
    it: [
      "Una licenza a pagamento acquistata prima del rilascio della versione 1.0 copre tutte le versioni 0.x e 1.x e gli aggiornamenti offerti per tali versioni.",
      "Una licenza a pagamento acquistata a partire dal rilascio della versione 1.0 copre la versione principale acquistata e gli aggiornamenti offerti per tale versione.",
      "Se viene offerta una versione principale successiva, il suo acquisto è facoltativo.",
      "Non acquistare una versione principale successiva non annulla il diritto di utilizzare le versioni già coperte dalla propria licenza.",
    ],
    ru: [
      "Платная лицензия, приобретённая до выхода версии 1.0, распространяется на все версии 0.x и 1.x и предоставляемые для них обновления.",
      "Платная лицензия, приобретённая со дня выхода версии 1.0, распространяется на приобретённую основную версию и предоставляемые для неё обновления.",
      "Если предлагается последующая основная версия, её покупка добровольна.",
      "Отказ от покупки последующей основной версии не отменяет право использовать версии, уже охватываемые вашей лицензией.",
    ],
  };
  assert.deepEqual(Object.keys(updateRights).sort(), languages.map(entry => entry.language).sort());
  for (const entry of languages) {
    const terms = readFileSync(new URL(`../${entry.appTerms}`, import.meta.url), "utf8");
    const purchases = terms.split(/^2\. /m)[1]?.split(/^3\. /m)[0]?.replace(/\s+/g, " ");
    assert.ok(purchases, `${entry.appTerms}: purchases and updates section`);
    for (const clause of updateRights[entry.language]) assert.ok(purchases.includes(clause), `${entry.appTerms}: ${clause}`);
    assert.doesNotMatch(purchases, /US\$\d+/, `${entry.appTerms}: agreement does not state prices`);
  }
});

test("required languages have complete distinct App and Driver contracts", () => {
  assert.deepEqual(languages.map(entry => entry.language).sort(), ["de", "en", "es", "fr", "it", "ja", "ko", "pt", "ru", "zh-Hans", "zh-Hant"]);
  for (const entry of languages) {
    for (const [key, sections, version] of [["appTerms", 10, "2026-10-31"], ["driverTerms", 8, "2026-10-10"]]) {
      const terms = readFileSync(new URL(`../${entry[key]}`, import.meta.url), "utf8");
      assert.deepEqual([...terms.matchAll(/^(\d+)\. /gm)].map(match => Number(match[1])), Array.from({ length: sections }, (_, i) => i + 1));
      for (const value of [version, "111-42-04359", "support@rawsector.com", "privacy@dsub.io", "legal@dsub.io", "+82 10-6380-9646", "Asia/Seoul"]) assert.ok(terms.includes(value), `${entry[key]}: ${value}`);
      assert.doesNotMatch(terms, /Professional legal review|review not completed|검토 미완료|Seunghyun Bae|배승현/);
    }
    const driver = readFileSync(new URL(`../${entry.driverTerms}`, import.meta.url), "utf8");
    assert.ok(driver.includes("LICENSE.apple-null-audio.txt"));
    assert.ok(driver.includes(entry.agreementTitle));
    assert.ok(driver.includes(entry.businessHeading));
  }
});

test("English product text uses British spelling", () => {
  // "license" stays only as a verb ("does not license a …"); file names such as LICENSE.txt are upper case.
  const american = /\b(?:[Ll]icenses?\b(?! a\b)|[Bb]ehaviors?\b|[Cc]olors?\b|[Cc]atalogs?\b|[Aa]nalyz(?:e|es|ed|ing)\b|[Cc]enters?\b|[Tt]oward\b|(?:[Ff]avo|[Hh]ono|[Ll]abo|[Nn]eighbo|[Hh]umo|[Rr]umo|[Ff]lavo|[Ee]ndeavo)r(?:s|ed|ing|able|ably|ite|ites|hood)?\b)/;
  const read = path => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
  const htmlText = source => source.replace(/<(script|style)[\s\S]*?<\/\1>/g, "")
    .replace(/<[^>]*\s(?:content|alt|title|aria-label)="([^"]*)"[^>]*>/g, " $1 ")
    .replace(/<[^>]*>/g, " ");
  const texts = [
    ...["LICENSE.txt", "LICENSE-Driver.txt", "apps/website/legal/en/LICENSE.txt", "apps/website/legal/en/LICENSE-Driver.txt"].map(path => [path, read(path)]),
    ...["terms.html", "privacy.html"].map(name => [`apps/website/legal/en/${name}`, htmlText(read(`apps/website/legal/en/${name}`))]),
    ...readdirSync(new URL("../apps/website/manual/en/", import.meta.url)).filter(name => name.endsWith(".html"))
      .map(name => [`apps/website/manual/en/${name}`, htmlText(read(`apps/website/manual/en/${name}`))]),
    ["apps/control-ui/src/core/i18n/locales/en.ts", [...read("apps/control-ui/src/core/i18n/locales/en.ts").matchAll(/'((?:[^'\\\n]|\\.)*)'/g)].map(match => match[1]).join("\n")],
    ["packaging/localization/languages.json", JSON.stringify(languages.find(entry => entry.language === "en"))],
    ["packaging/localization/linux-lifecycle.json", JSON.parse(read("packaging/localization/linux-lifecycle.json")).en.join("\n")],
  ];
  for (const [path, text] of texts) {
    const match = text.match(american);
    assert.equal(match, null, `${path}: American spelling "${match?.[0]}"`);
  }
});

test("macOS declares all UI languages and bundles localized permission descriptions", () => {
  const config = JSON.parse(readFileSync(new URL("../apps/desktop/src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  const plist = readFileSync(new URL("../apps/desktop/src-tauri/Info.plist", import.meta.url), "utf8");
  const localizations = plist.match(/<key>CFBundleLocalizations<\/key>\s*<array>([\s\S]*?)<\/array>/)?.[1];
  assert.ok(localizations);
  assert.deepEqual([...localizations.matchAll(/<string>(.*?)<\/string>/g)].map(match => match[1]).sort(), languages.map(entry => entry.language).sort());
  for (const {language} of languages) {
    const filename = `packaging/macos/localization/${language}.lproj/InfoPlist.strings`;
    const source = readFileSync(new URL(`../${filename}`, import.meta.url), "utf8");
    assert.match(source, /NSAudioCaptureUsageDescription = ".+";/);
    assert.match(source, /NSMicrophoneUsageDescription = ".+";/);
    assert.equal(config.bundle.resources[`../../../${filename}`], `${language}.lproj/InfoPlist.strings`);
  }
});

test("Linux removal selects actual translated messages and Chinese scripts", () => {
  const script = readFileSync(new URL("../packaging/linux/uninstall.sh", import.meta.url), "utf8");
  const start = script.indexOf('case "${LC_ALL:');
  const end = script.indexOf("\nesac", start) + 6;
  assert.ok(start >= 0 && end > start);
  for (const [locale, resolved] of [
    ["es_MX.UTF-8", "es"], ["pt_BR.UTF-8", "pt"], ["pt_PT.UTF-8", "pt"], ["it_IT.UTF-8", "it"], ["ru_RU.UTF-8", "ru"], ["ja_JP.UTF-8", "ja"], ["fr_CA.UTF-8", "fr"], ["de_DE.UTF-8", "de"],
    ["zh_CN.UTF-8", "zh-Hans"], ["zh_SG.UTF-8", "zh-Hans"],
    ["zh_TW.UTF-8", "zh-Hant"], ["zh_HK.UTF-8", "zh-Hant"],
    ["zh_Hans_TW", "zh-Hans"], ["zh-Hant-CN", "zh-Hant"],
  ]) {
    const expected = removal[resolved].app;
    const command = `${script.slice(start, end)}\nprintf '%s\\000' ${Object.keys(expected).map(key => `"$${key}"`).join(" ")}`;
    const result = spawnSync("/bin/sh", ["-c", command], { env: { PATH: "/usr/bin:/bin", LC_ALL: locale }, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(result.stdout.split("\0").slice(0, -1), Object.values(expected), `uninstall.sh: ${locale}`);
  }
  for (const locale of ["ko_KR.UTF-8", "en_US.UTF-8", "nl_NL.UTF-8"]) {
    const result = spawnSync("/bin/sh", ["-c", `${script.slice(start, end)}\nprintf '%s' "$title"`], { env: { LC_ALL: locale }, encoding: "utf8" });
    assert.equal(result.status, 0);
    assert.ok(result.stdout.includes("MixRelay"));
    assert.ok(locale.startsWith("ko") ? result.stdout.includes("제거") : result.stdout.includes("Uninstall"));
  }
});

test("Linux install/update/removal prerequisites use every supported language", () => {
  const messages = JSON.parse(readFileSync(new URL("../packaging/localization/linux-lifecycle.json", import.meta.url), "utf8"));
  for (const [index, filename] of ["driver-before-change.sh", "session-start.sh"].entries()) {
    const source = readFileSync(new URL(`../packaging/linux/${filename}`, import.meta.url), "utf8");
    const start = source.indexOf('case "${LC_ALL:');
    const end = source.indexOf("\nesac", start) + 6;
    for (const {language} of languages) {
      const result = spawnSync("/bin/sh", ["-c", `${source.slice(start, end)}\nprintf '%s' "$instruction"`], { env: { LC_ALL: language }, encoding: "utf8" });
      assert.equal(result.status, 0);
      assert.equal(result.stdout, messages[language][index]);
    }
    assert.match(source.slice(end), /printf '%s\\n' "\$instruction"/);
  }
});

test("all eleven App agreements identify Polar and link its buyer terms and privacy policy", () => {
  for (const entry of languages) {
    const terms = readFileSync(new URL(`../${entry.appTerms}`, import.meta.url), "utf8");
    assert.ok(terms.includes("Polar Software, Inc."), entry.appTerms);
    assert.ok(terms.includes("https://polar.sh/legal/checkout-buyer-terms"), entry.appTerms);
    assert.ok(terms.includes("https://polar.sh/legal/privacy-policy"), entry.appTerms);
    assert.ok(terms.includes("2026-10-31"), entry.appTerms);
    assert.doesNotMatch(terms, /lemon|squeezy/i, entry.appTerms);
  }
});
