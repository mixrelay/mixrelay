import { useTranslation } from 'react-i18next';

import { Panel } from '../core/ui/Panel';
import { StatusBadge } from '../core/ui/StatusBadge';
import { DriverStatusCard } from '../features/driver-status/DriverStatusCard';

export function StudioScreen() {
  const { t } = useTranslation();

  return (
    <main className="studio-shell">
      <header className="hero">
        <div className="brand-row">
          <div className="brand-mark" aria-hidden="true">TN</div>
          <span>{t('common.appName')}</span>
          <StatusBadge>{t('common.beta')}</StatusBadge>
        </div>
        <p className="eyebrow">{t('studio.eyebrow')}</p>
        <h1>{t('studio.title')}</h1>
        <p className="hero__description">{t('studio.description')}</p>
      </header>

      <section className="signal-path" aria-label="ToneNari signal path">
        <article>
          <span className="signal-path__index">01</span>
          <h2>{t('studio.capture')}</h2>
          <p>{t('studio.captureDescription')}</p>
        </article>
        <div className="signal-path__line" aria-hidden="true" />
        <article>
          <span className="signal-path__index">02</span>
          <h2>{t('studio.stream')}</h2>
          <p>{t('studio.streamDescription')}</p>
        </article>
      </section>

      <section className="dashboard-grid">
        <DriverStatusCard state="not_installed" />
        <Panel eyebrow="LAN" title={t('studio.remote')}>
          <p className="panel__description">{t('studio.remoteDescription')}</p>
          <StatusDotLabel label={t('common.ready')} />
        </Panel>
      </section>
    </main>
  );
}

function StatusDotLabel({ label }: { label: string }) {
  return (
    <span className="dot-label">
      <span aria-hidden="true" />
      {label}
    </span>
  );
}
