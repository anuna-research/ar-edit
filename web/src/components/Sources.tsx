import type { Source } from '../types';

interface SourcesProps {
  sources: Source[];
  selectedSourceId: string | null;
  onSelectSource: (id: string) => void;
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

export default function Sources({ sources, selectedSourceId, onSelectSource }: SourcesProps) {
  return (
    <div className="flex flex-col border border-neutral-700 rounded-md overflow-hidden">
      <div className="px-3 py-1.5 bg-neutral-800 border-b border-neutral-700 text-xs font-semibold uppercase tracking-wider text-neutral-400">
        Sources
      </div>
      {sources.length === 0 ? (
        <div className="flex-1 flex items-center justify-center text-neutral-500 text-sm p-4">
          No sources imported yet.
        </div>
      ) : (
        <div className="flex-1 overflow-y-auto">
          {sources.map((src) => {
            const selected = src.id === selectedSourceId;
            return (
              <button
                key={src.id}
                type="button"
                onClick={() => onSelectSource(src.id)}
                className={`w-full text-left px-3 py-2 border-b border-neutral-800 flex items-center gap-3 text-sm cursor-pointer transition-colors ${
                  selected
                    ? 'bg-blue-900/40 border-blue-700'
                    : 'hover:bg-neutral-800'
                }`}
              >
                <span className="text-neutral-400 font-mono text-xs shrink-0">
                  {src.id}
                </span>
                <span className="truncate flex-1 text-neutral-200">
                  {fileName(src.original_filename ?? src.path)}
                </span>
                <span className="text-neutral-500 text-xs shrink-0">
                  {formatDuration(src.duration_ms)}
                </span>
                <span className="shrink-0 flex gap-1 text-xs" title="transcript / index">
                  <span className={src.transcribed ? 'text-green-400' : 'text-neutral-600'}>
                    {src.transcribed ? '✓' : '◌'}
                  </span>
                  <span className={src.indexed ? 'text-green-400' : 'text-neutral-600'}>
                    {src.indexed ? '✓' : '◌'}
                  </span>
                </span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
