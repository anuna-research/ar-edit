import { useState, useRef, useCallback } from 'react';
import type { EditDocument, ShotRange, Source } from '../types';

// Consistent color palette for sources
const SOURCE_COLORS = [
  { bg: 'bg-blue-800/60', border: 'border-blue-500', text: 'text-blue-300' },
  { bg: 'bg-emerald-800/60', border: 'border-emerald-500', text: 'text-emerald-300' },
  { bg: 'bg-amber-800/60', border: 'border-amber-500', text: 'text-amber-300' },
  { bg: 'bg-purple-800/60', border: 'border-purple-500', text: 'text-purple-300' },
  { bg: 'bg-rose-800/60', border: 'border-rose-500', text: 'text-rose-300' },
  { bg: 'bg-cyan-800/60', border: 'border-cyan-500', text: 'text-cyan-300' },
  { bg: 'bg-orange-800/60', border: 'border-orange-500', text: 'text-orange-300' },
  { bg: 'bg-pink-800/60', border: 'border-pink-500', text: 'text-pink-300' },
];

function getShotDurationMs(range: ShotRange): number {
  switch (range.type) {
    case 'words':
      return (range.to - range.from + 1) * 150;
    case 'scenes':
      return (range.to - range.from + 1) * 5000;
    case 'time':
      return range.to_ms - range.from_ms;
  }
}

function formatDuration(ms: number): string {
  const totalSec = ms / 1000;
  if (totalSec < 60) return `${totalSec.toFixed(1)}s`;
  const min = Math.floor(totalSec / 60);
  const sec = totalSec % 60;
  return `${min}:${sec.toFixed(0).padStart(2, '0')}`;
}

function rangeLabel(range: ShotRange): string {
  switch (range.type) {
    case 'words':
      return `words ${range.from}-${range.to}`;
    case 'scenes':
      return `scenes ${range.from}-${range.to}`;
    case 'time':
      return `${formatDuration(range.from_ms)}-${formatDuration(range.to_ms)}`;
  }
}

interface TimelineProps {
  editDocument: EditDocument | null;
  selectedShotId: string | null;
  sources: Source[];
  onSelectShot: (id: string | null) => void;
  onReorderShot: (fromIndex: number, toIndex: number) => void;
  onPlayShot?: (shotId: string) => void;
}

export default function Timeline({
  editDocument,
  selectedShotId,
  sources: _sources,
  onSelectShot,
  onReorderShot,
  onPlayShot,
}: TimelineProps) {
  const [dragOverIndex, setDragOverIndex] = useState<number | null>(null);
  const dragIndexRef = useRef<number | null>(null);

  // Build a stable source->color map
  const sourceColorMap = new Map<string, (typeof SOURCE_COLORS)[number]>();
  const uniqueSourceIds = editDocument
    ? [...new Set(editDocument.shots.map((s) => s.source_id))]
    : [];
  uniqueSourceIds.forEach((id, i) => {
    sourceColorMap.set(id, SOURCE_COLORS[i % SOURCE_COLORS.length]);
  });

  const shots = editDocument?.shots ?? [];
  const durations = shots.map((s) => getShotDurationMs(s.range));
  const totalMs = durations.reduce((sum, d) => sum + d, 0);

  // Generate ruler marks
  const rulerMarks: number[] = [];
  if (totalMs > 0) {
    // Choose interval: aim for 5-15 marks
    let intervalMs = 5000;
    if (totalMs > 120_000) intervalMs = 30_000;
    else if (totalMs > 60_000) intervalMs = 10_000;
    else if (totalMs < 10_000) intervalMs = 1000;
    for (let t = 0; t <= totalMs; t += intervalMs) {
      rulerMarks.push(t);
    }
  }

  const handleDragStart = useCallback(
    (e: React.DragEvent, index: number) => {
      dragIndexRef.current = index;
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', String(index));
    },
    [],
  );

  const handleDragOver = useCallback(
    (e: React.DragEvent, index: number) => {
      e.preventDefault();
      e.dataTransfer.dropEffect = 'move';
      setDragOverIndex(index);
    },
    [],
  );

  const handleDrop = useCallback(
    (e: React.DragEvent, toIndex: number) => {
      e.preventDefault();
      const fromIndex = dragIndexRef.current;
      if (fromIndex !== null && fromIndex !== toIndex) {
        onReorderShot(fromIndex, toIndex);
      }
      dragIndexRef.current = null;
      setDragOverIndex(null);
    },
    [onReorderShot],
  );

  const handleDragEnd = useCallback(() => {
    dragIndexRef.current = null;
    setDragOverIndex(null);
  }, []);

  const MIN_SHOT_WIDTH_PX = 80;

  return (
    <div className="flex flex-col border border-neutral-700 rounded-md overflow-hidden">
      {/* Header */}
      <div className="px-3 py-1.5 bg-neutral-800 border-b border-neutral-700 text-xs font-semibold uppercase tracking-wider text-neutral-400 flex items-center justify-between">
        <span>Timeline</span>
        {totalMs > 0 && (
          <span className="text-neutral-500 normal-case tracking-normal font-normal">
            {shots.length} shot{shots.length !== 1 ? 's' : ''} &middot; {formatDuration(totalMs)}
          </span>
        )}
      </div>

      {shots.length === 0 ? (
        <div className="flex-1 flex items-center justify-center text-neutral-500 text-sm p-4">
          No shots yet. Add clips from Sources to build your timeline.
        </div>
      ) : (
        <div className="flex-1 flex flex-col min-h-0 overflow-auto p-2 gap-1.5">
          {/* Time ruler */}
          <div className="relative h-5 flex-shrink-0 border-b border-neutral-700/50 mb-1">
            {rulerMarks.map((t) => {
              const pct = totalMs > 0 ? (t / totalMs) * 100 : 0;
              return (
                <span
                  key={t}
                  className="absolute text-[10px] text-neutral-500 -translate-x-1/2"
                  style={{ left: `${pct}%` }}
                >
                  {formatDuration(t)}
                </span>
              );
            })}
          </div>

          {/* Shot blocks */}
          <div className="flex gap-0.5 items-stretch min-h-[56px]">
            {shots.map((shot, index) => {
              const durationMs = durations[index];
              const pct = totalMs > 0 ? (durationMs / totalMs) * 100 : 0;
              const colors = sourceColorMap.get(shot.source_id) ?? SOURCE_COLORS[0];
              const isSelected = shot.id === selectedShotId;
              const isDragOver = dragOverIndex === index;

              return (
                <div
                  key={shot.id}
                  className={[
                    'group relative flex flex-col justify-center px-2 py-1.5 rounded cursor-pointer border select-none transition-all',
                    colors.bg,
                    isSelected
                      ? `${colors.border} border-2 ring-1 ring-white/20`
                      : 'border-neutral-600/50 hover:border-neutral-500',
                    isDragOver ? 'ring-2 ring-blue-400/60' : '',
                  ].join(' ')}
                  style={{
                    flexBasis: `${pct}%`,
                    flexGrow: 0,
                    flexShrink: 0,
                    minWidth: `${MIN_SHOT_WIDTH_PX}px`,
                  }}
                  onClick={() => onSelectShot(shot.id)}
                  draggable
                  onDragStart={(e) => handleDragStart(e, index)}
                  onDragOver={(e) => handleDragOver(e, index)}
                  onDrop={(e) => handleDrop(e, index)}
                  onDragEnd={handleDragEnd}
                >
                  <div className={`text-xs font-medium truncate ${colors.text}`}>
                    {shot.source_id}
                  </div>
                  <div className="text-[10px] text-neutral-400 truncate">
                    {rangeLabel(shot.range)}
                  </div>
                  <div className="text-[10px] text-neutral-500 truncate">
                    {formatDuration(durationMs)}
                  </div>
                  {onPlayShot && (
                    <button
                      className="absolute top-1 right-1 w-5 h-5 flex items-center justify-center rounded bg-neutral-900/60 hover:bg-neutral-700 text-neutral-300 hover:text-white text-[10px] opacity-0 group-hover:opacity-100 transition-opacity"
                      title="Play shot"
                      onClick={(e) => {
                        e.stopPropagation();
                        onPlayShot(shot.id);
                      }}
                    >
                      &#9654;
                    </button>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
