import { useEffect, useRef, useState } from 'react';
import type { Source, Shot } from '../types';

interface SourceDetailProps {
  source: Source | null;
  /** If a shot is playing, jump to its time range */
  playingShot?: Shot | null;
  /** Current rotation in degrees (0/90/180/270) */
  rotation?: number;
  /** Cycle rotation to next value */
  onRotate?: () => void;
}

function formatDuration(ms?: number): string {
  if (ms == null) return '—';
  const totalSeconds = Math.floor(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}m ${seconds}s`;
}

function fileName(path: string): string {
  const parts = path.split('/');
  return parts[parts.length - 1] || path;
}

function StatusBadge({ label, value }: { label: string; value?: boolean }) {
  return (
    <div className="flex items-center gap-2 text-sm">
      <span className="text-neutral-400">{label}:</span>
      {value ? (
        <span className="text-green-400 font-medium">✓ Yes</span>
      ) : (
        <span className="text-neutral-500">◌ No</span>
      )}
    </div>
  );
}

export default function SourceDetail({ source, playingShot, rotation = 0, onRotate }: SourceDetailProps) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [videoError, setVideoError] = useState(false);
  const prevSourceId = useRef<string | null>(null);

  // Reset video error when source changes
  useEffect(() => {
    if (source?.id !== prevSourceId.current) {
      setVideoError(false);
      prevSourceId.current = source?.id ?? null;
    }
  }, [source?.id]);

  // Seek to shot time range when playingShot changes
  useEffect(() => {
    const video = videoRef.current;
    if (!video || !playingShot) return;

    if (playingShot.range.type === 'time') {
      video.currentTime = playingShot.range.from_ms / 1000;
      video.play().catch(() => {});
    }
  }, [playingShot]);

  return (
    <div className="flex flex-col border border-neutral-700 rounded-md overflow-hidden">
      <div className="px-3 py-1.5 bg-neutral-800 border-b border-neutral-700 text-xs font-semibold uppercase tracking-wider text-neutral-400">
        Preview
      </div>
      {!source ? (
        <div className="flex-1 flex items-center justify-center text-neutral-500 text-sm p-4">
          Select a source or shot to preview.
        </div>
      ) : (
        <div className="flex-1 overflow-y-auto p-3 space-y-3">
          {/* Video player */}
          <div className="relative rounded overflow-hidden bg-black aspect-video flex items-center justify-center">
            {videoError ? (
              <span className="text-neutral-600 text-sm">Cannot play video in browser</span>
            ) : (
              <video
                ref={videoRef}
                key={source.id}
                src={`/api/sources/${source.id}/video`}
                controls
                className="w-full h-full"
                style={{
                  transform: rotation ? `rotate(${rotation}deg)` : undefined,
                  ...(rotation === 90 || rotation === 270
                    ? { maxWidth: '100%', maxHeight: '100%', objectFit: 'contain', scale: 'calc(9/16)' }
                    : {}),
                  transition: 'transform 0.2s ease',
                }}
                onError={() => setVideoError(true)}
              />
            )}
            {onRotate && (
              <button
                className="absolute top-2 right-2 z-20 w-7 h-7 flex items-center justify-center rounded bg-neutral-900/70 hover:bg-neutral-700 text-neutral-300 hover:text-white text-sm transition-colors"
                title={`Rotate (currently ${rotation}\u00B0)`}
                onClick={(e) => {
                  e.stopPropagation();
                  onRotate();
                }}
              >
                &#x21bb;
              </button>
            )}
          </div>

          {/* Source info */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <span className="text-sm font-medium text-neutral-200 truncate">
                {fileName(source.original_filename ?? source.path)}
              </span>
              <span className="text-xs text-neutral-500 font-mono shrink-0 ml-2">{source.id}</span>
            </div>
            <div className="flex items-center gap-4 text-xs text-neutral-400">
              <span>{formatDuration(source.duration_ms)}</span>
              {source.resolution && <span>{source.resolution[0]}x{source.resolution[1]}</span>}
              {source.video_codec && <span>{source.video_codec}</span>}
            </div>
            <div className="flex gap-3">
              <StatusBadge label="Transcript" value={source.transcribed} />
              <StatusBadge label="Index" value={source.indexed} />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
