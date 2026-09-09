import { Group, Paper, Title } from '@mantine/core';
import type { PropsWithChildren, ReactNode } from 'react';

import styles from './Panel.module.css';

interface PanelProps extends PropsWithChildren {
  title: string;
  trailing?: ReactNode;
}

export function Panel({ children, title, trailing }: PanelProps) {
  return (
    <Paper className={styles.root} component="section" p="xl" withBorder>
      <Group align="flex-start" className={styles.header} justify="space-between" wrap="nowrap">
        <Title className={styles.title} order={2}>
          {title}
        </Title>
        {trailing}
      </Group>
      {children}
    </Paper>
  );
}
