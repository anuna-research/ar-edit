import { useState, useRef, useCallback, useEffect, useMemo } from 'react';
import type { EditDocument, ShotRange, Source } from '../types';
import ContextMenu from './ContextMenu';
import type { ContextMenuItem } from './ContextMenu';

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

const MS_PER_WORD = 150;
const MS_PER_SCENE = 5000;
const MIN_CLIP_PX = 60;
// Default and range for zoom (pixels per second of content)
const DEFAULT_PX_PER_SEC = 40;
const MIN_PX_PER_SEC = 5;
const MAX_PX_PER_SEC = 300;
// Width of edge drag handle zones in pixels
const EDGE_HANDLE_PX = 4;

function getShotDurationMs(range: ShotRange, _source?: Source): number {
  switch (range.type) {
    case 'time':
      return range.to_ms - range.from_ms;
    case 'words':
      return (range.to - range.from + 1) * MS_PER_WORD;
    case 'scenes':
      return (range.to - range.from + 1) * MS_PER_SCENE;
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

/** Compute px-per-ms ratio for a clip given its width and duration */
function clipPxPerMs(widthPx: number, durationMs: number): number {
  return durationMs > 0 ? widthPx / durationMs : 0;
}

interface EdgeDragState {
  shotId: string;
  edge: 'left' | 'right';
  startX: number;
  originalRange: ShotRange;
  pxPerMs: number;
}

interface ContextMenuState {
  x: number;
  y: number;
  shotId: string;
  /** The time (ms) or unit index within the clip where the right-click occurred */
  splitMs: number;
}

interface TrimTooltipState {
  x: number;
  y: number;
  label: string;
}

interface TimelineProps {
  editDocument: EditDocument | null;
  selectedShotId: string | null;
  sources: Source[];
  onSelectShot: (id: string | null) => void;
  onReorderShot: (fromIndex: number, toIndex: number) => void;
  onPlayShot?: (shotId: string) => void;
  onTrimShot?: (shotId: string, newRange: ShotRange) => void;
  onSplitShot?: (shotId: string, splitMs: number) => void;
  onDeleteShot?: (shotId: string) => void;
  /** Map of source ID to rotation degrees (0/90/180/270) */
  rotations?: Map<string, number>;
}

export default function Timeline({
  editDocument,
  selectedShotId,
  sources,
  onSelectShot,
  onReorderShot,
  onPlayShot,
  onTrimShot,
  onSplitShot,
  onDeleteShot,
  rotations,
}: TimelineProps) {
  const [dragOverIndex, setDragOverIndex] = useState<number | null>(null);
  const dragIndexRef = useRef<number | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [containerWidth, setContainerWidth] = useState(0);
  const [failedThumbnails, setFailedThumbnails] = useState<Set<string>>(new Set());
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [trimTooltip, setTrimTooltip] = useState<TrimTooltipState | null>(null);
  const [pxPerSec, setPxPerSec] = useState(DEFAULT_PX_PER_SEC);
  const [playheadMs, setPlayheadMs] = useState<number | null>(null);
  const rulerRef = useRef<HTMLDivElement>(null);

  // Edge-drag state stored in ref (no re-renders during drag — tooltip is separate)
  const edgeDragRef = useRef<EdgeDragState | null>(null);

  // Ctrl+scroll / pinch to zoom on timeline
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const handler = (e: WheelEvent) => {
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        const factor = e.deltaY > 0 ? 0.9 : 1.1;
        setPxPerSec((v) => Math.min(MAX_PX_PER_SEC, Math.max(MIN_PX_PER_SEC, v * factor)));
      }
    };
    el.addEventListener('wheel', handler, { passive: false });
    return () => el.removeEventListener('wheel', handler);
  }, []);

  // Track container width for proportional pixel calculations
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setContainerWidth(entry.contentRect.width);
      }
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // Build source lookup
  const sourceMap = useMemo(() => {
    const m = new Map<string, Source>();
    for (const s of sources) m.set(s.id, s);
    return m;
  }, [sources]);

  // Build a stable source->color map
  const sourceColorMap = useMemo(() => {
    const map = new Map<string, (typeof SOURCE_COLORS)[number]>();
    const ids = editDocument
      ? [...new Set(editDocument.shots.map((s) => s.source_id))]
      : [];
    ids.forEach((id, i) => {
      map.set(id, SOURCE_COLORS[i % SOURCE_COLORS.length]);
    });
    return map;
  }, [editDocument]);

  const shots = editDocument?.shots ?? [];

  const durations = useMemo(
    () => shots.map((s) => getShotDurationMs(s.range, sourceMap.get(s.source_id))),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [shots, sourceMap],
  );

  const totalMs = useMemo(
    () => durations.reduce((sum, d) => sum + d, 0),
    [durations],
  );

  // Move playhead to start of selected shot
  useEffect(() => {
    if (!selectedShotId || !editDocument) return;
    const idx = editDocument.shots.findIndex((s) => s.id === selectedShotId);
    if (idx === -1) return;
    let elapsed = 0;
    for (let i = 0; i < idx; i++) {
      elapsed += durations[i] ?? 0;
    }
    setPlayheadMs(elapsed);
  }, [selectedShotId, editDocument, durations]);

  // Compute per-clip pixel widths and total scrollable content width
  const { clipWidths, contentWidth } = useMemo(() => {
    if (totalMs === 0 || shots.length === 0) {
      return { clipWidths: [] as number[], contentWidth: 0 };
    }

    // Base width: time-proportional at current zoom, but never narrower than the viewport
    const naturalWidth = Math.max((totalMs / 1000) * pxPerSec, containerWidth);

    // First pass: proportional widths
    const proportional = durations.map((d) => (d / totalMs) * naturalWidth);

    // Second pass: enforce minimum and redistribute deficit
    let deficit = 0;
    let surplusTotal = 0;
    const widths = proportional.map((w) => {
      if (w < MIN_CLIP_PX) {
        deficit += MIN_CLIP_PX - w;
        return MIN_CLIP_PX;
      }
      surplusTotal += w;
      return w;
    });

    if (deficit > 0 && surplusTotal > 0) {
      const scale = Math.max(0, 1 - deficit / surplusTotal);
      for (let i = 0; i < widths.length; i++) {
        if (proportional[i] >= MIN_CLIP_PX) {
          widths[i] = Math.max(MIN_CLIP_PX, widths[i] * scale);
        }
      }
    }

    const total = widths.reduce((a, b) => a + b, 0);
    return { clipWidths: widths, contentWidth: total };
  }, [totalMs, durations, shots.length, containerWidth, pxPerSec]);

  // Cumulative pixel offsets for ruler alignment
  const clipOffsets = useMemo(() => {
    const offsets: number[] = [];
    let acc = 0;
    for (const w of clipWidths) {
      offsets.push(acc);
      acc += w;
    }
    return offsets;
  }, [clipWidths]);

  // Map a time (ms) to a pixel position, interpolating within clips
  const timeToPx = useCallback(
    (timeMs: number): number => {
      if (totalMs === 0) return 0;
      let elapsed = 0;
      for (let i = 0; i < durations.length; i++) {
        const d = durations[i];
        if (elapsed + d >= timeMs) {
          const frac = d > 0 ? (timeMs - elapsed) / d : 0;
          return clipOffsets[i] + frac * clipWidths[i];
        }
        elapsed += d;
      }
      return contentWidth;
    },
    [totalMs, durations, clipOffsets, clipWidths, contentWidth],
  );

  // Map a pixel position back to a time (ms) — inverse of timeToPx
  const pxToTime = useCallback(
    (px: number): number => {
      if (totalMs === 0) return 0;
      let elapsed = 0;
      for (let i = 0; i < clipWidths.length; i++) {
        const w = clipWidths[i];
        const offset = clipOffsets[i];
        if (px <= offset + w) {
          const frac = w > 0 ? (px - offset) / w : 0;
          return elapsed + frac * durations[i];
        }
        elapsed += durations[i];
      }
      return totalMs;
    },
    [totalMs, clipWidths, clipOffsets, durations],
  );

  // Handle ruler click to place playhead
  const handleRulerClick = useCallback(
    (e: React.MouseEvent) => {
      const ruler = rulerRef.current;
      if (!ruler) return;
      const rect = ruler.getBoundingClientRect();
      const scrollLeft = ruler.parentElement?.parentElement?.scrollLeft ?? 0;
      const localX = e.clientX - rect.left + scrollLeft;
      setPlayheadMs(pxToTime(localX));
    },
    [pxToTime],
  );

  // Generate ruler marks
  const rulerMarks = useMemo(() => {
    const marks: number[] = [];
    if (totalMs <= 0) return marks;
    let intervalMs = 5000;
    if (totalMs > 120_000) intervalMs = 30_000;
    else if (totalMs > 60_000) intervalMs = 10_000;
    else if (totalMs < 10_000) intervalMs = 1000;
    for (let t = 0; t <= totalMs; t += intervalMs) {
      marks.push(t);
    }
    return marks;
  }, [totalMs]);

  // ── Drag-to-reorder handlers ──────────────────────────────────────────

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

  // ── Edge-drag (trim) handlers ─────────────────────────────────────────

  /** Compute a new range from a pixel delta during edge drag */
  const computeTrimmedRange = useCallback(
    (state: EdgeDragState, deltaX: number): ShotRange | null => {
      const { edge, originalRange, pxPerMs } = state;
      if (pxPerMs === 0) return null;
      const deltaMs = deltaX / pxPerMs;

      switch (originalRange.type) {
        case 'time': {
          let from = originalRange.from_ms;
          let to = originalRange.to_ms;
          if (edge === 'left') {
            from = Math.max(0, Math.round(from + deltaMs));
            if (from >= to) from = to - 1;
          } else {
            to = Math.round(to + deltaMs);
            if (to <= from) to = from + 1;
          }
          return { type: 'time', from_ms: from, to_ms: to };
        }
        case 'words': {
          const deltaWords = Math.round(deltaMs / MS_PER_WORD);
          let from = originalRange.from;
          let to = originalRange.to;
          if (edge === 'left') {
            from = Math.max(0, from + deltaWords);
            if (from > to) from = to;
          } else {
            to = to + deltaWords;
            if (to < from) to = from;
          }
          return { type: 'words', from, to };
        }
        case 'scenes': {
          const deltaScenes = Math.round(deltaMs / MS_PER_SCENE);
          let from = originalRange.from;
          let to = originalRange.to;
          if (edge === 'left') {
            from = Math.max(0, from + deltaScenes);
            if (from > to) from = to;
          } else {
            to = to + deltaScenes;
            if (to < from) to = from;
          }
          return { type: 'scenes', from, to };
        }
      }
    },
    [],
  );

  const handleEdgeMouseDown = useCallback(
    (
      e: React.MouseEvent,
      shotId: string,
      edge: 'left' | 'right',
      range: ShotRange,
      widthPx: number,
      durationMs: number,
    ) => {
      e.stopPropagation();
      e.preventDefault();
      edgeDragRef.current = {
        shotId,
        edge,
        startX: e.clientX,
        originalRange: range,
        pxPerMs: clipPxPerMs(widthPx, durationMs),
      };
    },
    [],
  );

  // Global mousemove/mouseup for edge drag
  useEffect(() => {
    function onMouseMove(e: MouseEvent) {
      const state = edgeDragRef.current;
      if (!state) return;
      const deltaX = e.clientX - state.startX;
      const newRange = computeTrimmedRange(state, deltaX);
      if (newRange) {
        // Live-update the clip size as you drag
        if (onTrimShot) {
          onTrimShot(state.shotId, newRange);
        }
        setTrimTooltip({
          x: e.clientX,
          y: e.clientY - 28,
          label: rangeLabel(newRange),
        });
      }
    }
    function onMouseUp() {
      edgeDragRef.current = null;
      setTrimTooltip(null);
    }
    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
    return () => {
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup', onMouseUp);
    };
  }, [computeTrimmedRange, onTrimShot]);

  // ── Context menu (right-click split/delete) ───────────────────────────

  const handleContextMenu = useCallback(
    (e: React.MouseEvent, shotId: string, range: ShotRange, widthPx: number) => {
      e.preventDefault();
      e.stopPropagation();

      // Compute the time within the clip where the user clicked
      const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
      const localX = e.clientX - rect.left;
      const fraction = localX / widthPx;

      let splitMs: number;
      switch (range.type) {
        case 'time':
          splitMs = range.from_ms + fraction * (range.to_ms - range.from_ms);
          splitMs = Math.round(splitMs);
          break;
        case 'words':
          splitMs = range.from + fraction * (range.to - range.from + 1);
          splitMs = Math.round(splitMs);
          break;
        case 'scenes':
          splitMs = range.from + fraction * (range.to - range.from + 1);
          splitMs = Math.round(splitMs);
          break;
      }

      setContextMenu({ x: e.clientX, y: e.clientY, shotId, splitMs });
    },
    [],
  );

  const contextMenuItems = useMemo((): ContextMenuItem[] => {
    if (!contextMenu) return [];
    const items: ContextMenuItem[] = [];
    if (onSplitShot) {
      items.push({
        label: 'Split here',
        onClick: () => onSplitShot(contextMenu.shotId, contextMenu.splitMs),
      });
    }
    if (onDeleteShot) {
      items.push({
        label: 'Delete clip',
        onClick: () => onDeleteShot(contextMenu.shotId),
      });
    }
    return items;
  }, [contextMenu, onSplitShot, onDeleteShot]);

  return (
    <div className="flex flex-col border border-neutral-700 rounded-md overflow-hidden">
      {/* Header */}
      <div className="px-3 py-1.5 bg-neutral-800 border-b border-neutral-700 text-xs font-semibold uppercase tracking-wider text-neutral-400 flex items-center justify-between">
        <span>Timeline</span>
        <div className="flex items-center gap-2">
          {totalMs > 0 && (
            <span className="text-neutral-500 normal-case tracking-normal font-normal mr-2">
              {shots.length} shot{shots.length !== 1 ? 's' : ''} &middot; {formatDuration(totalMs)}
            </span>
          )}
          <button
            className="w-5 h-5 flex items-center justify-center rounded bg-neutral-700 hover:bg-neutral-600 text-neutral-300 text-xs"
            title="Zoom out"
            onClick={() => setPxPerSec((v) => Math.max(MIN_PX_PER_SEC, v / 1.3))}
          >
            −
          </button>
          <input
            type="range"
            min={Math.log(MIN_PX_PER_SEC)}
            max={Math.log(MAX_PX_PER_SEC)}
            step={0.01}
            value={Math.log(pxPerSec)}
            onChange={(e) => setPxPerSec(Math.exp(Number(e.target.value)))}
            className="w-20 h-1 accent-neutral-500"
            title={`Zoom: ${Math.round(pxPerSec)}px/s`}
          />
          <button
            className="w-5 h-5 flex items-center justify-center rounded bg-neutral-700 hover:bg-neutral-600 text-neutral-300 text-xs"
            title="Zoom in"
            onClick={() => setPxPerSec((v) => Math.min(MAX_PX_PER_SEC, v * 1.3))}
          >
            +
          </button>
          <button
            className="px-1.5 h-5 flex items-center justify-center rounded bg-neutral-700 hover:bg-neutral-600 text-neutral-400 text-[10px] normal-case tracking-normal font-normal"
            title="Fit to view"
            onClick={() => {
              if (totalMs > 0 && containerWidth > 0) {
                setPxPerSec(Math.max(MIN_PX_PER_SEC, (containerWidth / totalMs) * 1000 * 0.95));
              }
            }}
          >
            Fit
          </button>
        </div>
      </div>

      {shots.length === 0 ? (
        <div className="flex-1 flex items-center justify-center text-neutral-500 text-sm p-4">
          No shots yet. Add clips from Sources to build your timeline.
        </div>
      ) : (
        <div
          ref={containerRef}
          className="flex-1 min-h-0 overflow-x-auto overflow-y-hidden p-2"
        >
          {/* Inner wrapper sized to the full scrollable width */}
          <div className="relative" style={{ width: contentWidth > 0 ? `${contentWidth}px` : '100%' }}>
            {/* Time ruler — clickable to place playhead */}
            <div
              ref={rulerRef}
              className="relative h-6 border-b border-neutral-700/50 mb-1 cursor-pointer"
              onClick={handleRulerClick}
            >
              {rulerMarks.map((t) => {
                const px = timeToPx(t);
                return (
                  <div key={t} className="absolute" style={{ left: `${px}px` }}>
                    {/* Tick mark */}
                    <div className="absolute bottom-0 w-px h-2 bg-neutral-600" />
                    {/* Label */}
                    <span
                      className="absolute bottom-2 text-[10px] text-neutral-500 whitespace-nowrap"
                      style={{ transform: 'translateX(-50%)' }}
                    >
                      {formatDuration(t)}
                    </span>
                  </div>
                );
              })}
              {/* Sub-ticks between main marks */}
              {rulerMarks.length >= 2 && (() => {
                const interval = rulerMarks[1] - rulerMarks[0];
                const subInterval = interval / 4;
                const ticks: React.ReactNode[] = [];
                for (let t = 0; t <= totalMs; t += subInterval) {
                  // Skip positions that coincide with main marks
                  if (t % interval === 0) continue;
                  const px = timeToPx(t);
                  ticks.push(
                    <div
                      key={`sub-${t}`}
                      className="absolute bottom-0 w-px h-1 bg-neutral-700"
                      style={{ left: `${px}px` }}
                    />
                  );
                }
                return ticks;
              })()}
            </div>

            {/* Playhead line — spans ruler + clips */}
            {playheadMs !== null && (
              <div
                className="absolute top-0 bottom-0 w-px bg-red-500 z-30 pointer-events-none"
                style={{ left: `${timeToPx(playheadMs)}px` }}
              >
                {/* Playhead handle (triangle at top) */}
                <div className="absolute -top-0.5 -translate-x-1/2 w-0 h-0"
                  style={{
                    borderLeft: '5px solid transparent',
                    borderRight: '5px solid transparent',
                    borderTop: '6px solid #ef4444',
                  }}
                />
              </div>
            )}

            {/* Shot blocks — single horizontal row */}
            <div className="flex items-stretch min-h-[56px]">
              {shots.map((shot, index) => {
                const durationMs = durations[index];
                const width = clipWidths[index] ?? MIN_CLIP_PX;
                const colors = sourceColorMap.get(shot.source_id) ?? SOURCE_COLORS[0];
                const isSelected = shot.id === selectedShotId;
                const isDragOver = dragOverIndex === index;

                const fullLabel = `${shot.source_id} | ${rangeLabel(shot.range)} | ${formatDuration(durationMs)}`;
                const hasThumbnail = !failedThumbnails.has(shot.source_id);
                const thumbnailUrl = `/api/sources/${encodeURIComponent(shot.source_id)}/thumbnail`;
                const thumbRotation = rotations?.get(shot.source_id) ?? 0;

                return (
                  <div
                    key={shot.id}
                    className={[
                      'group relative flex flex-col justify-center px-2 py-1.5 rounded cursor-pointer border select-none transition-all overflow-hidden flex-shrink-0 flex-grow-0',
                      !hasThumbnail ? colors.bg : '',
                      isSelected
                        ? `${colors.border} border-2 ring-1 ring-white/20`
                        : 'border-neutral-600/50 hover:border-neutral-500',
                      isDragOver ? 'ring-2 ring-blue-400/60' : '',
                    ].join(' ')}
                    style={{ width: `${width}px` }}
                    title={fullLabel}
                    onClick={() => onSelectShot(shot.id)}
                    draggable
                    onDragStart={(e) => handleDragStart(e, index)}
                    onDragOver={(e) => handleDragOver(e, index)}
                    onDrop={(e) => handleDrop(e, index)}
                    onDragEnd={handleDragEnd}
                    onContextMenu={(e) =>
                      handleContextMenu(e, shot.id, shot.range, width)
                    }
                  >
                    {/* Thumbnail background with rotation support */}
                    {hasThumbnail && (
                      <div
                        className="absolute inset-0 rounded"
                        style={{
                          backgroundImage: `url(${thumbnailUrl})`,
                          backgroundSize: 'cover',
                          backgroundPosition: 'center',
                          transform: thumbRotation ? `rotate(${thumbRotation}deg)` : undefined,
                          ...(thumbRotation === 90 || thumbRotation === 270
                            ? { transformOrigin: 'center center', scale: '1.4' }
                            : {}),
                        }}
                      />
                    )}
                    {/* Hidden img to detect thumbnail load failure */}
                    {hasThumbnail && (
                      <img
                        src={thumbnailUrl}
                        alt=""
                        className="hidden"
                        onError={() =>
                          setFailedThumbnails((prev) => new Set(prev).add(shot.source_id))
                        }
                      />
                    )}
                    {/* Dark overlay for text readability when thumbnail is shown */}
                    {hasThumbnail && (
                      <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-black/50 to-black/30 rounded" />
                    )}

                    {/* Left edge drag handle */}
                    <div
                      className="absolute left-0 top-0 bottom-0 z-20 opacity-0 group-hover:opacity-100 hover:!opacity-100 bg-white/30 transition-opacity"
                      style={{ width: `${EDGE_HANDLE_PX}px`, cursor: 'col-resize' }}
                      onMouseDown={(e) =>
                        handleEdgeMouseDown(e, shot.id, 'left', shot.range, width, durationMs)
                      }
                    />
                    {/* Right edge drag handle */}
                    <div
                      className="absolute right-0 top-0 bottom-0 z-20 opacity-0 group-hover:opacity-100 hover:!opacity-100 bg-white/30 transition-opacity"
                      style={{ width: `${EDGE_HANDLE_PX}px`, cursor: 'col-resize' }}
                      onMouseDown={(e) =>
                        handleEdgeMouseDown(e, shot.id, 'right', shot.range, width, durationMs)
                      }
                    />

                    <div className={`relative z-10 text-xs font-medium truncate ${hasThumbnail ? 'text-white drop-shadow-md' : colors.text}`}>
                      {shot.source_id}
                    </div>
                    <div className={`relative z-10 text-[10px] truncate ${hasThumbnail ? 'text-neutral-200 drop-shadow-md' : 'text-neutral-400'}`}>
                      {rangeLabel(shot.range)}
                    </div>
                    <div className={`relative z-10 text-[10px] truncate ${hasThumbnail ? 'text-neutral-300 drop-shadow-md' : 'text-neutral-500'}`}>
                      {formatDuration(durationMs)}
                    </div>
                    {onPlayShot && (
                      <button
                        className="absolute top-1 right-1 z-10 w-5 h-5 flex items-center justify-center rounded bg-neutral-900/60 hover:bg-neutral-700 text-neutral-300 hover:text-white text-[10px] opacity-0 group-hover:opacity-100 transition-opacity"
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
        </div>
      )}

      {/* Trim tooltip shown during edge drag */}
      {trimTooltip && (
        <div
          className="fixed z-50 px-2 py-0.5 bg-neutral-900 border border-neutral-600 rounded text-xs text-neutral-200 whitespace-nowrap pointer-events-none"
          style={{ left: trimTooltip.x, top: trimTooltip.y, transform: 'translateX(-50%)' }}
        >
          {trimTooltip.label}
        </div>
      )}

      {/* Context menu */}
      {contextMenu && contextMenuItems.length > 0 && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={contextMenuItems}
          onClose={() => setContextMenu(null)}
        />
      )}
    </div>
  );
}
