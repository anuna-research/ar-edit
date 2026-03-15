interface StatusBarProps {
  projectTitle: string | null;
  selectedShotId: string | null;
  shotCount: number;
}

export default function StatusBar({
  projectTitle,
  selectedShotId,
  shotCount,
}: StatusBarProps) {
  return (
    <div className="flex items-center px-3 py-1 bg-neutral-800 border-t border-neutral-700 text-xs text-neutral-400 gap-4">
      {/* Project title */}
      <span className="font-medium text-neutral-300">
        {projectTitle ?? 'Loading...'}
      </span>

      {/* Mode / selected shot indicator */}
      <span className="text-neutral-500">
        {selectedShotId
          ? `Shot ${selectedShotId} selected`
          : shotCount > 0
            ? `${shotCount} shot${shotCount !== 1 ? 's' : ''}`
            : 'No shots'}
      </span>

      {/* Keyboard shortcut hints */}
      <span className="ml-auto text-neutral-500 hidden sm:inline">
        <kbd className="px-1 py-0.5 bg-neutral-700 rounded text-neutral-300 text-[10px]">p</kbd>{' '}
        play{' '}
        <kbd className="px-1 py-0.5 bg-neutral-700 rounded text-neutral-300 text-[10px]">j</kbd>/
        <kbd className="px-1 py-0.5 bg-neutral-700 rounded text-neutral-300 text-[10px]">k</kbd>{' '}
        navigate{' '}
        <kbd className="px-1 py-0.5 bg-neutral-700 rounded text-neutral-300 text-[10px]">Esc</kbd>{' '}
        deselect
      </span>
    </div>
  );
}
