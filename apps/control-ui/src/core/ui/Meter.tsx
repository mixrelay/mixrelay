interface MeterProps {
  label: string;
  level: number;
}

export function Meter({ label, level }: MeterProps) {
  return (
    <span className="meter" title={`${label}: ${level}%`}>
      <span className="meter__fill" style={{ height: `${level}%` }} />
    </span>
  );
}
