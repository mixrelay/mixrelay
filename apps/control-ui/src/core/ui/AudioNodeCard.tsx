import { Paper, UnstyledButton } from '@mantine/core';

import { StatusBadge, type StatusTone } from './StatusBadge';

const portIds = ['one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight'];

export interface AudioNodeCardProps {
  bypassed?: boolean;
  category: string;
  detail: string;
  icon: string;
  inputCount: number;
  outputCount: number;
  selected?: boolean;
  status?: string;
  statusTone?: StatusTone;
  title: string;
  onSelect: () => void;
}

export function AudioNodeCard({
  bypassed = false,
  category,
  detail,
  icon,
  inputCount,
  outputCount,
  onSelect,
  selected = false,
  status,
  statusTone = 'neutral',
  title,
}: AudioNodeCardProps) {
  return (
    <Paper
      className="audio-node"
      data-bypassed={bypassed || undefined}
      data-selected={selected || undefined}
      withBorder
    >
      <UnstyledButton
        aria-label={`${title}, ${detail}`}
        aria-pressed={selected}
        className="audio-node__button"
        onClick={onSelect}
      >
        <span className="audio-node__topline">
          <span className="audio-node__icon" aria-hidden="true">
            {icon}
          </span>
          <span className="audio-node__category">{category}</span>
          {status ? <StatusBadge tone={statusTone}>{status}</StatusBadge> : null}
        </span>
        <strong>{title}</strong>
        <small>{detail}</small>
      </UnstyledButton>
      <NodePorts count={inputCount} side="input" />
      <NodePorts count={outputCount} side="output" />
    </Paper>
  );
}

function NodePorts({ count, side }: { count: number; side: 'input' | 'output' }) {
  return (
    <span aria-hidden="true" className={`audio-node__ports audio-node__ports--${side}`}>
      {portIds.slice(0, count).map((portId) => (
        <span key={`${side}-${portId}`} />
      ))}
    </span>
  );
}
