import { useState, useCallback, useEffect } from 'react';
import Timeline from './components/Timeline';
import Transcript from './components/Transcript';
import Sources from './components/Sources';
import SourceDetail from './components/SourceDetail';
import StatusBar from './components/StatusBar';
import { useProject } from './hooks/useProject';
import { useRotation } from './hooks/useRotation';
import { useKeyboard } from './hooks/useKeyboard';
import type { Shot } from './types';

function App() {
  const [selectedSourceId, setSelectedSourceId] = useState<string | null>(null);
  const [playingShot, setPlayingShot] = useState<Shot | null>(null);

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

  // Auto-select the source when a shot is selected
  useEffect(() => {
    if (selectedShot) {
      setSelectedSourceId(selectedShot.source_id);
    }
  }, [selectedShot]);

  useKeyboard({ editDocument, selectedShotId, selectShot });

  const handlePlayShot = useCallback(
    (shotId: string) => {
      if (!editDocument) return;
      const shot = editDocument.shots.find((s) => s.id === shotId);
      if (shot) {
        setSelectedSourceId(shot.source_id);
        setPlayingShot(shot);
      }
    },
    [editDocument],
  );

  // Show the shot's source in the preview, or manually selected source
  const previewSource = sources.find((s) => s.id === selectedSourceId) ?? null;

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
        />
        <Transcript transcript={transcript} selectedShot={selectedShot} />
        <Sources
          sources={sources}
          selectedSourceId={selectedSourceId}
          onSelectSource={setSelectedSourceId}
        />
        <SourceDetail
          source={previewSource}
          playingShot={playingShot}
          rotation={previewSource ? getRotation(previewSource.id) : 0}
          onRotate={previewSource ? () => cycleRotation(previewSource.id) : undefined}
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
