import { useEffect, useRef } from 'react';
import type { Transcript as TranscriptType, Shot } from '../types';
import { flattenWords } from '../types';

interface TranscriptProps {
  transcript?: TranscriptType | null;
  selectedShot?: Shot | null;
}

function formatMs(ms: number): string {
  const totalSec = Math.floor(ms / 1000);
  const m = Math.floor(totalSec / 60);
  const s = totalSec % 60;
  return `${m}:${String(s).padStart(2, '0')}`;
}

export default function Transcript({ transcript, selectedShot }: TranscriptProps) {
  const highlightRef = useRef<HTMLSpanElement | null>(null);

  useEffect(() => {
    if (highlightRef.current) {
      highlightRef.current.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }
  }, [transcript, selectedShot]);

  let body: React.ReactNode;

  if (!selectedShot) {
    body = (
      <div className="flex-1 flex items-center justify-center text-neutral-500 text-sm p-4">
        Select a shot to view transcript
      </div>
    );
  } else if (!transcript) {
    body = (
      <div className="flex-1 flex items-center justify-center text-neutral-500 text-sm p-4">
        Loading transcript...
      </div>
    );
  } else if (selectedShot.range.type === 'scenes') {
    const range = selectedShot.range;
    body = (
      <div className="flex-1 flex items-center justify-center text-neutral-400 text-sm p-4">
        Scene range: scenes {range.from}&ndash;{range.to}
      </div>
    );
  } else if (selectedShot.range.type === 'words') {
    const range = selectedShot.range;
    let firstHighlightRef = false;
    body = (
      <div className="flex-1 overflow-y-auto p-4 text-sm leading-relaxed">
        <p className="whitespace-pre-wrap">
          {flattenWords(transcript).map((word, i) => {
            const inRange = i >= range.from && i <= range.to;
            const showIndex = i % 10 === 0;
            let ref: React.Ref<HTMLSpanElement> | undefined;
            if (inRange && !firstHighlightRef) {
              ref = highlightRef;
              firstHighlightRef = true;
            }
            return (
              <span key={i}>
                {showIndex && (
                  <sup className="text-[10px] text-neutral-600 mr-0.5 select-none">{i}</sup>
                )}
                <span
                  ref={ref}
                  className={inRange ? 'bg-amber-500/30 rounded-sm px-0.5' : undefined}
                >
                  {word.text}
                </span>{' '}
              </span>
            );
          })}
        </p>
      </div>
    );
  } else {
    // time range
    const range = selectedShot.range;
    let firstHighlightRef = false;
    body = (
      <div className="flex-1 overflow-y-auto p-4 text-sm leading-relaxed">
        <p className="whitespace-pre-wrap">
          {flattenWords(transcript).map((word, i) => {
            const inRange = word.start_ms >= range.from_ms && word.start_ms <= range.to_ms;
            const showTimestamp = i % 10 === 0;
            let ref: React.Ref<HTMLSpanElement> | undefined;
            if (inRange && !firstHighlightRef) {
              ref = highlightRef;
              firstHighlightRef = true;
            }
            return (
              <span key={i}>
                {showTimestamp && (
                  <sup className="text-[10px] text-neutral-600 mr-0.5 select-none">
                    {formatMs(word.start_ms)}
                  </sup>
                )}
                <span
                  ref={ref}
                  className={inRange ? 'bg-amber-500/30 rounded-sm px-0.5' : undefined}
                >
                  {word.text}
                </span>{' '}
              </span>
            );
          })}
        </p>
      </div>
    );
  }

  return (
    <div className="flex flex-col border border-neutral-700 rounded-md overflow-hidden">
      <div className="px-3 py-1.5 bg-neutral-800 border-b border-neutral-700 text-xs font-semibold uppercase tracking-wider text-neutral-400">
        Transcript
      </div>
      {body}
    </div>
  );
}
