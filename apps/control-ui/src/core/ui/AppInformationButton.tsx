import { Button } from 'react-aria-components';
import styles from './AppInformationButton.module.css';
import { Tooltip } from './Tooltip';

/** One status action; its label and dialog relationship describe the workflow it opens. */
export function AppInformationButton({
  label,
  version,
  status,
  opened,
  onOpen,
}: {
  label: string;
  version: string;
  status: string;
  opened: boolean;
  onOpen: () => void;
}) {
  const summary = `${version} · ${status}`;
  return (
    <Tooltip label={`${label} · ${summary}`}>
      <Button
        className={styles.root}
        render={(props) => <button {...props} aria-description={summary} />}
        aria-label={label}
        aria-description={summary}
        aria-expanded={opened}
        aria-haspopup="dialog"
        onClick={onOpen}
      >
        <span className={styles.summary}>
          <span className={styles.version}>{version}</span>
          <span className={styles.separator} aria-hidden="true">
            {' · '}
          </span>
          <span className={styles.status}>{status}</span>
        </span>
      </Button>
    </Tooltip>
  );
}
