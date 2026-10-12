import { useTranslation } from 'react-i18next';
import { resolveLocale } from '../../core/i18n';
import plexLicense from '../../core/theme/fonts/OFL.txt?raw';
import { appLicenseTerms } from './appLicenseTerms';
import styles from './LicenseAccess.module.css';

/** Legal reference only; opening it does not accept terms or change activation. */
export function LicenseTerms() {
  const { i18n, t } = useTranslation();
  return (
    <div className={styles.terms}>
      <details>
        <summary>{t('licensing.termsTitle')}</summary>
        <p>{t('licensing.integrationNotice')}</p>
        <pre className={styles.termsText} lang={resolveLocale(i18n.language)}>
          {appLicenseTerms(i18n.language)}
        </pre>
      </details>
      <details>
        <summary>IBM Plex · SIL Open Font License 1.1</summary>
        <pre className={styles.termsText} lang="en">
          {plexLicense}
        </pre>
      </details>
    </div>
  );
}
