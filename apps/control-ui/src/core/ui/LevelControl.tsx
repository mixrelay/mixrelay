import { Slider, Stack, Text } from '@mantine/core';

import styles from './LevelControl.module.css';

interface LevelControlProps {
  label: string;
  max: number;
  min: number;
  onChange: (value: number) => void;
  suffix: string;
  value: number;
}

export function LevelControl({ label, max, min, onChange, suffix, value }: LevelControlProps) {
  return (
    <Stack className={styles.root} gap={8}>
      <span className={styles.label}>
        <Text size="sm">{label}</Text>
        <Text c="dimmed" ff="monospace" size="xs">
          {value} {suffix}
        </Text>
      </span>
      <Slider
        aria-label={label}
        color="signal"
        label={null}
        max={max}
        min={min}
        onChange={onChange}
        value={value}
      />
    </Stack>
  );
}
