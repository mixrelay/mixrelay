import { act, fireEvent, render } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import de from '../../../../../LICENSE.de.txt?raw';
import es from '../../../../../LICENSE.es.txt?raw';
import fr from '../../../../../LICENSE.fr.txt?raw';
import itTerms from '../../../../../LICENSE.it.txt?raw';
import ja from '../../../../../LICENSE.ja.txt?raw';
import ko from '../../../../../LICENSE.ko.txt?raw';
import pt from '../../../../../LICENSE.pt.txt?raw';
import ru from '../../../../../LICENSE.ru.txt?raw';
import en from '../../../../../LICENSE.txt?raw';
import zhHans from '../../../../../LICENSE.zh-Hans.txt?raw';
import zhHant from '../../../../../LICENSE.zh-Hant.txt?raw';
import { I18nProvider, i18n } from '../../core/i18n';
import { LicenseTerms } from './LicenseTerms';

afterEach(async () => {
  await i18n.changeLanguage('en');
});

describe('localized App license presentation', () => {
  it.each([
    ['en', en],
    ['ko-KR', ko],
    ['ja', ja],
    ['zh-Hans', zhHans],
    ['zh-Hant', zhHant],
    ['fr', fr],
    ['de', de],
    ['es', es],
    ['pt', pt],
    ['it', itTerms],
    ['ru', ru],
  ])('shows the complete %s contract in the disclosure', async (locale, source) => {
    await i18n.changeLanguage(locale);
    const { container } = render(
      <I18nProvider>
        <LicenseTerms />
      </I18nProvider>,
    );
    const disclosure = container.querySelector('details');
    expect(disclosure).not.toHaveAttribute('open');
    const summary = container.querySelector('summary');
    if (!summary) throw new Error('License disclosure summary missing');
    fireEvent.click(summary);
    const displayed = container.querySelector('pre');
    expect(displayed?.textContent).toBe(source);
    expect(displayed).toHaveAttribute('lang', locale);
    expect(source.match(/^\d+\. /gm)).toHaveLength(10);
    const purchases = source.split(/^2\. /m)[1]?.split(/^3\. /m)[0];
    for (const version of ['1.0', '0.x', '1.x']) {
      expect(purchases).toContain(version);
    }
    expect(source).toContain('2026-10-31');
    expect(source).toContain('111-42-04359');
    expect(source).toContain('https://appserver.rawsector.com');
    expect(source).toContain('https://polar.sh/legal/privacy-policy');
    expect(source).toContain('support@rawsector.com');
  });

  it('changes an already open contract when the app language changes', async () => {
    await i18n.changeLanguage('en');
    const { container } = render(
      <I18nProvider>
        <LicenseTerms />
      </I18nProvider>,
    );
    container.querySelector('details')?.setAttribute('open', '');
    expect(container.querySelector('pre')?.textContent).toBe(en);
    await act(async () => {
      await i18n.changeLanguage('ko-KR');
    });
    expect(container.querySelector('details')).toHaveAttribute('open');
    expect(container.querySelector('pre')?.textContent).toBe(ko);
    expect(container.querySelector('pre')).toHaveAttribute('lang', 'ko-KR');
  });
});
