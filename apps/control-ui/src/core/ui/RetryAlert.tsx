import { Button } from '@mantine/core';

/** A recoverable operation failure; retry never discards the caller's last valid data. */
export function RetryAlert({
  className,
  message,
  retryLabel,
  busy,
  onRetry,
}: {
  className?: string;
  message: string;
  retryLabel: string;
  busy: boolean;
  onRetry: () => void;
}) {
  return (
    <div className={className} role="alert">
      {message}{' '}
      <Button size="xs" variant="subtle" disabled={busy} onClick={onRetry}>
        {retryLabel}
      </Button>
    </div>
  );
}
