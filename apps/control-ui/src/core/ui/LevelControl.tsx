import { Slider, Stack, Text } from '@mantine/core';

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
    <Stack className="level-control" gap={8}>
      <span className="level-control__label">
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
