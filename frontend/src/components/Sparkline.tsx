interface Point {
  date: string;
  value: number;
}

interface Props {
  points: Point[];
  width?: number;
  height?: number;
  stroke?: string;
}

/** Lightweight pure-SVG sparkline (no chart library). */
export default function Sparkline({
  points,
  width = 320,
  height = 80,
  stroke = "#34d399",
}: Props) {
  if (points.length === 0) {
    return (
      <p className="py-4 text-center text-sm text-neutral-500">暂无历史数据</p>
    );
  }
  const values = points.map((p) => p.value);
  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = max - min || 1;
  const pad = 4;
  const coords = points.map((p, i) => {
    const x =
      points.length === 1
        ? width / 2
        : pad + (i / (points.length - 1)) * (width - pad * 2);
    const y = height - pad - ((p.value - min) / span) * (height - pad * 2);
    return `${x},${y}`;
  });
  const last = points[points.length - 1];
  const first = points[0];

  return (
    <div className="space-y-1">
      <svg
        viewBox={`0 0 ${width} ${height}`}
        className="w-full h-20"
        role="img"
        aria-label={`history from ${first.date} to ${last.date}`}
      >
        <polyline
          fill="none"
          stroke={stroke}
          strokeWidth="2"
          points={coords.join(" ")}
        />
        {points.length === 1 && (
          <circle cx={width / 2} cy={height / 2} r="3" fill={stroke} />
        )}
      </svg>
      <div className="flex justify-between text-xs text-neutral-500">
        <span>
          {first.date} · {first.value.toLocaleString()}
        </span>
        <span>
          {last.date} · {last.value.toLocaleString()}
        </span>
      </div>
    </div>
  );
}
