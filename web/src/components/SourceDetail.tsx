import { useEffect, useRef, useState, useCallback, useImperativeHandle, forwardRef } from 'react';
import type { Source } from '../types';

/** Imperative API for controlling playback from parent */
export interface VideoHandle {
  /** Seek to a source-local time and optionally play. Returns true if same-source (instant). */
  seekAndPlay: (sourceId: string, sec: number, play: boolean) => boolean;
}

interface SourceDetailProps {
  source: Source | null;
  seekToSec?: number | null;
  autoPlay?: boolean;
  onTimeUpdate?: (sec: number) => void;
  onPlayStateChange?: (playing: boolean) => void;
  rotation?: number;
  onRotate?: () => void;
  nextSourceId?: string | null;
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

const SourceDetail = forwardRef<VideoHandle, SourceDetailProps>(function SourceDetail({
  source,
  seekToSec,
  autoPlay,
  onTimeUpdate,
  onPlayStateChange,
  rotation = 0,
  onRotate,
  nextSourceId,
}, ref) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [videoError, setVideoError] = useState(false);
  const seekingRef = useRef(false);
  const pendingRef = useRef<{ sec: number; play: boolean } | null>(null);
  const currentSrcRef = useRef<string | null>(null);

  // Imperative API: seek directly on the video element (no state roundtrip)
  useImperativeHandle(ref, () => ({
    seekAndPlay(sourceId: string, sec: number, play: boolean): boolean {
      const video = videoRef.current;
      if (!video) return false;
      const expectedSrc = `/api/sources/${sourceId}/video`;
      if (currentSrcRef.current === expectedSrc && video.readyState >= 2) {
        // Same source, already loaded — instant seek
        seekingRef.current = true;
        video.currentTime = sec;
        if (play && video.paused) video.play().catch(() => {});
        setTimeout(() => { seekingRef.current = false; }, 50);
        return true;
      }
      // Different source — need to go through state
      return false;
    },
  }), []);

  // Change video source without remounting the element
  useEffect(() => {
    const video = videoRef.current;
    if (!video || !source) return;
    const newSrc = `/api/sources/${source.id}/video`;
    if (currentSrcRef.current !== newSrc) {
      currentSrcRef.current = newSrc;
      video.src = newSrc;
      video.load();
      setVideoError(false);
    }
  }, [source?.id]);

  // React to seek commands
  useEffect(() => {
    if (seekToSec == null) return;
    const video = videoRef.current;
    if (!video) return;

    const doSeek = () => {
      seekingRef.current = true;
      video.currentTime = seekToSec;
      if (autoPlay) {
        video.play().catch(() => {});
      } else if (!autoPlay && !video.paused) {
        video.pause();
      }
      setTimeout(() => { seekingRef.current = false; }, 50);
    };

    if (video.readyState >= 2) {
      doSeek();
    } else {
      // Defer until video is ready
      pendingRef.current = { sec: seekToSec, play: !!autoPlay };
    }
  }, [seekToSec, autoPlay]);

  const handleCanPlay = useCallback(() => {
    const pending = pendingRef.current;
    if (!pending) return;
    pendingRef.current = null;
    const video = videoRef.current;
    if (!video) return;
    seekingRef.current = true;
    video.currentTime = pending.sec;
    if (pending.play) {
      video.play().catch(() => {});
    }
    setTimeout(() => { seekingRef.current = false; }, 50);
  }, []);

  const handleTimeUpdate = useCallback(() => {
    const video = videoRef.current;
    if (!video || seekingRef.current) return;
    onTimeUpdate?.(video.currentTime);
  }, [onTimeUpdate]);

  const handlePlay = useCallback(() => onPlayStateChange?.(true), [onPlayStateChange]);
  const handlePause = useCallback(() => onPlayStateChange?.(false), [onPlayStateChange]);

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
                preload="auto"
                controls
                className="w-full h-full"
                style={{
                  transform: rotation ? `rotate(${rotation}deg)` : undefined,
                  ...(rotation === 90 || rotation === 270
                    ? { maxWidth: '100%', maxHeight: '100%', objectFit: 'contain' as const, scale: 'calc(9/16)' }
                    : {}),
                  transition: 'transform 0.2s ease',
                }}
                onError={() => setVideoError(true)}
                onCanPlay={handleCanPlay}
                onTimeUpdate={handleTimeUpdate}
                onPlay={handlePlay}
                onPause={handlePause}
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

          {/* Preload next source video (hidden) */}
          {nextSourceId && nextSourceId !== source.id && (
            <link
              rel="preload"
              href={`/api/sources/${nextSourceId}/video`}
              as="video"
            />
          )}

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
});

export default SourceDetail;
