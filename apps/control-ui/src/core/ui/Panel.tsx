import { Group, Paper, Stack, Text, Title } from '@mantine/core';
import type { PropsWithChildren, ReactNode } from 'react';

interface PanelProps extends PropsWithChildren {
  eyebrow?: string;
  title: string;
  trailing?: ReactNode;
}

export function Panel({ children, eyebrow, title, trailing }: PanelProps) {
  return (
    <Paper className="panel" component="section" p="xl" withBorder>
      <Group align="flex-start" className="panel__header" justify="space-between" wrap="nowrap">
        <Stack gap={0}>
          {eyebrow ? <Text className="eyebrow">{eyebrow}</Text> : null}
          <Title order={2}>{title}</Title>
        </Stack>
        {trailing}
      </Group>
      {children}
    </Paper>
  );
}
