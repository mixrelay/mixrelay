import { type ReactNode, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { ActionButton } from '../../core/ui/ActionButton';
import { UpdatesDialog } from '../../core/ui/UpdatesDialog';
import type {
  ProductUpdate,
  UpdateDownload,
  UpdateProduct,
  UpdatesPort,
} from '../../core/updates/UpdatesPort';
import styles from './UpdateAccess.module.css';
import { useUpdatesSession } from './useUpdatesSession';

const products: UpdateProduct[] = ['mix-relay', 'mix-relay-driver'];

function downloadLabel(download: UpdateDownload) {
  const extension = new URL(download.url).pathname.match(/\.(pkg|dmg|deb|rpm|appimage)$/i)?.[1];
  return `${extension ? extension.toUpperCase() : download.os} · ${download.arch}`;
}

function ProductRelease({
  product,
  update,
  session,
}: {
  product: UpdateProduct;
  update: ProductUpdate | undefined;
  session: ReturnType<typeof useUpdatesSession>;
}) {
  const { t } = useTranslation();
  const state = session.failed ? 'unavailable' : (update?.state ?? 'unavailable');
  const downloads =
    state === 'unavailable' || state === 'not_published' ? [] : (update?.downloads ?? []);
  const name = product === 'mix-relay' ? 'MixRelay' : 'MixRelay Driver';
  return (
    <section className={styles.product} aria-label={name}>
      <h2>{name}</h2>
      <p className={styles.versions}>
        {t('updates.installed', { version: update?.installedVersion ?? t('updates.unknown') })}
      </p>
      {update?.latestVersion ? (
        <p className={styles.versions}>{t('updates.latest', { version: update.latestVersion })}</p>
      ) : null}
      {session.checked ? <p>{t(`updates.states.${state}`)}</p> : null}
      <div className={styles.downloads}>
        {downloads.map((download) => (
          <ActionButton
            key={download.url}
            disabled={session.checking || session.opening !== null}
            onClick={() => void session.openDownload(product, download.url)}
            size="xs"
            variant="light"
          >
            {t('updates.openDownload', { asset: downloadLabel(download) })}
          </ActionButton>
        ))}
      </div>
    </section>
  );
}

/** The update dialog stays outside the presentation licensing gate. */
export function UpdateAccess({
  children,
  port,
}: {
  children: (action: ReactNode) => ReactNode;
  port: UpdatesPort | null;
}) {
  const { t } = useTranslation();
  const [opened, setOpened] = useState(false);
  const session = useUpdatesSession(port);
  const available = session.products.some((product) => product.state === 'available');
  const actionLabel = t(available ? 'updates.available' : 'updates.check');
  const action = port ? (
    <ActionButton
      aria-label={actionLabel}
      title={actionLabel}
      aria-expanded={opened}
      aria-haspopup="dialog"
      className={styles.action}
      variant="subtle"
      size="xs"
      onClick={() => {
        setOpened(true);
        void session.refresh();
      }}
    >
      <svg aria-hidden="true" viewBox="0 0 16 16">
        <path d="M8 2v8m-3-3 3 3 3-3M3 11v3h10v-3" />
      </svg>
      {available ? <span className={styles.available} aria-hidden="true" /> : null}
    </ActionButton>
  ) : null;

  return (
    <>
      {children(action)}
      <UpdatesDialog
        opened={opened && port !== null}
        onClose={() => setOpened(false)}
        closeLabel={t('updates.close')}
        title={t('updates.title')}
      >
        {session.checking ? <p role="status">{t('updates.checking')}</p> : null}
        {products.map((product) => (
          <ProductRelease
            key={product}
            product={product}
            session={session}
            update={session.products.find((update) => update.product === product)}
          />
        ))}
        {session.openFailed ? <p role="alert">{t('updates.openFailed')}</p> : null}
        <p className={styles.notice}>{t('updates.installNotice')}</p>
        <div className={styles.footer}>
          <ActionButton
            disabled={session.checking || session.opening !== null}
            size="xs"
            variant="subtle"
            onClick={() => void session.refresh()}
          >
            {t('updates.checkAgain')}
          </ActionButton>
        </div>
      </UpdatesDialog>
    </>
  );
}
