import de from '../../../../../LICENSE.de.txt?raw';
import es from '../../../../../LICENSE.es.txt?raw';
import fr from '../../../../../LICENSE.fr.txt?raw';
import it from '../../../../../LICENSE.it.txt?raw';
import ja from '../../../../../LICENSE.ja.txt?raw';
import ko from '../../../../../LICENSE.ko.txt?raw';
import pt from '../../../../../LICENSE.pt.txt?raw';
import ru from '../../../../../LICENSE.ru.txt?raw';
import en from '../../../../../LICENSE.txt?raw';
import zhHans from '../../../../../LICENSE.zh-Hans.txt?raw';
import zhHant from '../../../../../LICENSE.zh-Hant.txt?raw';
import { resolveLocale, type SupportedLocale } from '../../core/i18n';

const terms: Record<SupportedLocale, string> = {
  en,
  'ko-KR': ko,
  ja,
  'zh-Hans': zhHans,
  'zh-Hant': zhHant,
  fr,
  de,
  es,
  pt,
  it,
  ru,
};

export function appLicenseTerms(language: string): string {
  return terms[resolveLocale(language)];
}
