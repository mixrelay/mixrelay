import { Button, Group } from '@mantine/core';

export function RecoveryActions({
  retryLabel,
  reloadLabel,
  onRetry,
  onReload,
}: {
  retryLabel: string;
  reloadLabel: string;
  onRetry: () => void;
  onReload: () => void;
}) {
  return (
    <Group gap="xs" mt="xs">
      <Button size="xs" variant="light" onClick={onRetry}>
        {retryLabel}
      </Button>
      <Button size="xs" variant="subtle" onClick={onReload}>
        {reloadLabel}
      </Button>
    </Group>
  );
}
