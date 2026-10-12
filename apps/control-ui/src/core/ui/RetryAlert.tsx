import { ActionButton } from './ActionButton';
import { InlineNotice } from './InlineNotice';

/** A recoverable operation failure; retry never discards the caller's last valid data. */
export function RetryAlert({
  framed = false,
  message,
  retryLabel,
  busy,
  onRetry,
}: {
  framed?: boolean;
  message: string;
  retryLabel: string;
  busy: boolean;
  onRetry: () => void;
}) {
  const content = (
    <>
      {message}{' '}
      <ActionButton disabled={busy} onClick={onRetry}>
        {retryLabel}
      </ActionButton>
    </>
  );
  return framed ? (
    <InlineNotice layout="banner" tone="critical">
      {content}
    </InlineNotice>
  ) : (
    <div role="alert">{content}</div>
  );
}
