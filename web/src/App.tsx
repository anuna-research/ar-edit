import { useState, useCallback, useEffect, useRef } from 'react';
import Timeline from './components/Timeline';
import Transcript from './components/Transcript';
import Sources from './components/Sources';
import SourceDetail from './components/SourceDetail';
import type { VideoHandle } from './components/SourceDetail';
import StatusBar from './components/StatusBar';
import { useProject } from './hooks/useProject';
import { useRotation } from './hooks/useRotation';
import { useKeyboard } from './hooks/useKeyboard';
import type { Shot, ShotRange } from './types';

const MS_PER_WORD = 150;
const MS_PER_SCENE = 5000;

function shotDurationMs(range: ShotRange): number {
  switch (range.type) {
    case 'time': return range.to_ms - range.from_ms;
    case 'words': return (range.to - range.from + 1) * MS_PER_WORD;
    case 'scenes': return (range.to - range.from + 1) * MS_PER_SCENE;
  }
}

function resolvePlayhead(shots: Shot[], globalMs: number): { shot: Shot; offsetMs: number; index: number } | null {
  let elapsed = 0;
  for (let i = 0; i < shots.length; i++) {
    const dur = shotDurationMs(shots[i].range);
    if (globalMs < elapsed + dur) {
      return { shot: shots[i], offsetMs: globalMs - elapsed, index: i };
    }
    elapsed += dur;
  }
  if (shots.length > 0) {
    const last = shots[shots.length - 1];
    return { shot: last, offsetMs: shotDurationMs(last.range), index: shots.length - 1 };
  }
  return null;
}

/** Convert a shot-local offset (ms) to source-local seconds */
function shotOffsetToSourceSec(shot: Shot, offsetMs: number): number {
  if (shot.range.type === 'time') {
    return (shot.range.from_ms + offsetMs) / 1000;
  }
  return offsetMs / 1000;
}

/** Convert source-local seconds to shot-local offset ms */
function sourceSecToShotOffsetMs(shot: Shot, sec: number): number {
  if (shot.range.type === 'time') {
    return sec * 1000 - shot.range.from_ms;
  }
  return sec * 1000;
}

/** Source-local end time in seconds for a shot */
function shotEndSourceSec(shot: Shot): number {
  if (shot.range.type === 'time') {
    return shot.range.to_ms / 1000;
  }
  return shotDurationMs(shot.range) / 1000;
}

function shotToGlobalMs(shots: Shot[], shotId: string, offsetMs: number): number {
  let elapsed = 0;
  for (const s of shots) {
    if (s.id === shotId) return elapsed + Math.max(0, offsetMs);
    elapsed += shotDurationMs(s.range);
  }
  return elapsed;
}

function shotStartGlobalMs(shots: Shot[], index: number): number {
  let elapsed = 0;
  for (let i = 0; i < index; i++) {
    elapsed += shotDurationMs(shots[i].range);
  }
  return elapsed;
}

function App() {
  const [selectedSourceId, setSelectedSourceId] = useState<string | null>(null);
  const [playheadMs, setPlayheadMs] = useState<number | null>(null);
  const [seekToSec, setSeekToSec] = useState<number | null>(null);
  const [autoPlay, setAutoPlay] = useState(false);
  const [isPlaying, setIsPlaying] = useState(false);
  const activeShot = useRef<Shot | null>(null);
  const activeIndex = useRef<number>(-1);
  const isPlayingRef = useRef(false);
  const seekCounter = useRef(0);
  const videoHandle = useRef<VideoHandle>(null);

  const {
    editDocument,
    selectedShotId,
    selectShot,
    sources,
    transcript,
    reorderShot,
    trimShot,
    splitShot,
    deleteShot,
    projectTitle,
  } = useProject();

  const { rotations, getRotation, cycleRotation } = useRotation(sources);

  const selectedShot = editDocument?.shots.find((s) => s.id === selectedShotId) ?? null;
  const shots = editDocument?.shots ?? [];

  useEffect(() => {
    if (selectedShot) {
      setSelectedSourceId(selectedShot.source_id);
    }
  }, [selectedShot]);

  // Navigate to a shot and optionally play it
  const goToShot = useCallback(
    (shot: Shot, index: number, offsetMs = 0, play = false) => {
      activeShot.current = shot;
      activeIndex.current = index;
      selectShot(shot.id);

      const sec = shotOffsetToSourceSec(shot, offsetMs);

      // Fast path: try imperative seek (instant for same-source)
      if (videoHandle.current?.seekAndPlay(shot.source_id, sec, play)) {
        // Instant seek succeeded — no state update needed for video
        setSelectedSourceId(shot.source_id);
        return;
      }

      // Slow path: different source, need state-driven source change
      setSelectedSourceId(shot.source_id);
      seekCounter.current += 1;
      setSeekToSec(sec + seekCounter.current * 1e-6);
      setAutoPlay(play);
    },
    [selectShot],
  );

  // Play/pause toggle
  const handleTogglePlay = useCallback(() => {
    if (isPlaying) {
      setIsPlaying(false);
      isPlayingRef.current = false;
      setAutoPlay(false);
      return;
    }
    const startMs = playheadMs ?? 0;
    const resolved = resolvePlayhead(shots, startMs);
    if (!resolved) return;

    setIsPlaying(true);
    isPlayingRef.current = true;
    goToShot(resolved.shot, resolved.index, resolved.offsetMs, true);
  }, [isPlaying, playheadMs, shots, goToShot]);

  useKeyboard({ editDocument, selectedShotId, selectShot, onTogglePlay: handleTogglePlay });

  // Play a single shot (▶ button on clip)
  const handlePlayShot = useCallback(
    (shotId: string) => {
      const idx = shots.findIndex((s) => s.id === shotId);
      if (idx === -1) return;
      setIsPlaying(true);
      isPlayingRef.current = true;
      goToShot(shots[idx], idx, 0, true);
    },
    [shots, goToShot],
  );

  // Playhead scrub → seek video, stop playing
  const handlePlayheadChange = useCallback(
    (ms: number) => {
      setPlayheadMs(ms);
      setIsPlaying(false);
      isPlayingRef.current = false;
      const resolved = resolvePlayhead(shots, ms);
      if (resolved) {
        goToShot(resolved.shot, resolved.index, resolved.offsetMs, false);
      }
    },
    [shots, goToShot],
  );

  // Keep a ref to shots so the RAF loop always sees current data
  const shotsRef = useRef(shots);
  shotsRef.current = shots;

  // RAF-based playback loop: polls video.currentTime at ~60fps,
  // updates the playhead, and handles shot transitions instantly.
  const rafId = useRef<number>(0);

  useEffect(() => {
    if (!isPlaying) {
      if (rafId.current) cancelAnimationFrame(rafId.current);
      return;
    }

    const tick = () => {
      const video = videoHandle.current;
      const shot = activeShot.current;
      const allShots = shotsRef.current;

      if (!shot || !video) {
        rafId.current = requestAnimationFrame(tick);
        return;
      }

      // Get current time directly from the video element
      // Direct access to the video element for zero-latency seeks
      const videoEl = (document.querySelector('video') as HTMLVideoElement | null);
      if (!videoEl) {
        rafId.current = requestAnimationFrame(tick);
        return;
      }

      const sec = videoEl.currentTime;
      const endSec = shotEndSourceSec(shot);

      if (sec >= endSec - 0.02) {
        // Shot boundary reached — advance immediately
        const nextIdx = activeIndex.current + 1;
        if (nextIdx < allShots.length) {
          const nextShot = allShots[nextIdx];
          activeShot.current = nextShot;
          activeIndex.current = nextIdx;

          const nextSec = shotOffsetToSourceSec(nextShot, 0);

          // Same source? Seek directly on the element — zero latency
          if (nextShot.source_id === shot.source_id) {
            videoEl.currentTime = nextSec;
            // Video is already playing, no need to call play()
          } else {
            // Different source — must go through state
            setSelectedSourceId(nextShot.source_id);
            seekCounter.current += 1;
            setSeekToSec(nextSec + seekCounter.current * 1e-6);
            setAutoPlay(true);
          }

          selectShot(nextShot.id);
          setPlayheadMs(shotStartGlobalMs(allShots, nextIdx));
        } else {
          // End of edit
          videoEl.pause();
          setIsPlaying(false);
          isPlayingRef.current = false;
          setAutoPlay(false);
        }
      } else {
        // Normal: update playhead position
        const offsetMs = sourceSecToShotOffsetMs(shot, sec);
        const dur = shotDurationMs(shot.range);
        const clamped = Math.min(Math.max(0, offsetMs), dur);
        const globalMs = shotToGlobalMs(allShots, shot.id, clamped);
        setPlayheadMs(globalMs);
      }

      rafId.current = requestAnimationFrame(tick);
    };

    rafId.current = requestAnimationFrame(tick);
    return () => { if (rafId.current) cancelAnimationFrame(rafId.current); };
  }, [isPlaying, selectShot]);

  // When video is paused via native controls
  const handlePlayStateChange = useCallback((playing: boolean) => {
    if (!playing && isPlayingRef.current) {
      setIsPlaying(false);
      isPlayingRef.current = false;
    }
    if (playing && !isPlayingRef.current) {
      setIsPlaying(true);
      isPlayingRef.current = true;
    }
  }, []);

  const previewSource = sources.find((s) => s.id === selectedSourceId) ?? null;

  // Figure out the next shot's source for preloading
  const nextSourceId = (() => {
    const idx = activeIndex.current;
    if (idx >= 0 && idx + 1 < shots.length) {
      return shots[idx + 1].source_id;
    }
    return null;
  })();

  return (
    <div className="h-full flex flex-col bg-neutral-900 text-neutral-200">
      <div className="flex-1 grid grid-cols-[3fr_2fr] grid-rows-[1fr_1fr] gap-1 p-1 min-h-0">
        <Timeline
          editDocument={editDocument}
          selectedShotId={selectedShotId}
          sources={sources}
          onSelectShot={selectShot}
          onReorderShot={reorderShot}
          onPlayShot={handlePlayShot}
          onTrimShot={trimShot}
          onSplitShot={splitShot}
          onDeleteShot={deleteShot}
          rotations={rotations}
          playheadMs={playheadMs}
          onPlayheadChange={handlePlayheadChange}
          isPlaying={isPlaying}
          onTogglePlay={handleTogglePlay}
        />
        <Transcript transcript={transcript} selectedShot={selectedShot} />
        <Sources
          sources={sources}
          selectedSourceId={selectedSourceId}
          onSelectSource={setSelectedSourceId}
        />
        <SourceDetail
          ref={videoHandle}
          source={previewSource}
          seekToSec={seekToSec}
          autoPlay={autoPlay}
          onPlayStateChange={handlePlayStateChange}
          rotation={previewSource ? getRotation(previewSource.id) : 0}
          onRotate={previewSource ? () => cycleRotation(previewSource.id) : undefined}
          nextSourceId={nextSourceId}
        />
      </div>
      <StatusBar
        projectTitle={projectTitle}
        selectedShotId={selectedShotId}
        shotCount={editDocument?.shots.length ?? 0}
      />
    </div>
  );
}

export default App;
