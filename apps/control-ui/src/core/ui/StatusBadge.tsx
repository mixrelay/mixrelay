import { Badge } from '@mantine/core';
import type { PropsWithChildren } from 'react';

export type StatusTone = 'neutral' | 'positive' | 'warning' | 'critical';

interface StatusBadgeProps extends PropsWithChildren {
  className?: string;
  tone?: StatusTone;
}

export function StatusBadge({ children, className, tone = 'neutral' }: StatusBadgeProps) {
  const colors: Record<StatusTone, string> = {
    neutral: 'gray',
    positive: 'healthy',
    warning: 'yellow',
    critical: 'red',
  };

  return (
    <Badge
      className={className}
      color={colors[tone]}
      radius={999}
      size="sm"
      styles={{
        label: { lineHeight: 1.25, whiteSpace: 'normal' },
        root: { height: 'auto', maxWidth: '100%', minHeight: '1.75rem' },
      }}
      variant="light"
    >
      {children}
    </Badge>
  );
}
