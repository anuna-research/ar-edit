import { useEffect, useState, useCallback } from 'react';
import type { EditDocument, EditDocumentRaw, Source, ShotRange, Transcript, Manifest } from '../types';
import { normaliseShot } from '../types';

export interface UseProjectReturn {
  editDocument: EditDocument | null;
  editNames: string[];
  selectedShotId: string | null;
  selectShot: (id: string | null) => void;
  sources: Source[];
  selectedSource: Source | undefined;
  transcript: Transcript | null;
  reorderShot: (fromIndex: number, toIndex: number) => void;
  trimShot: (shotId: string, newRange: ShotRange) => void;
  splitShot: (shotId: string, splitMs: number) => void;
  deleteShot: (shotId: string) => void;
  projectTitle: string | null;
}

export function useProject(): UseProjectReturn {
  const [projectTitle, setProjectTitle] = useState<string | null>(null);
  const [editNames, setEditNames] = useState<string[]>([]);
  const [editName, setEditName] = useState<string | null>(null);
  const [editDocument, setEditDocument] = useState<EditDocument | null>(null);
  const [selectedShotId, setSelectedShotId] = useState<string | null>(null);
  const [sources, setSources] = useState<Source[]>([]);
  const [transcript, setTranscript] = useState<Transcript | null>(null);

  // Fetch manifest
  useEffect(() => {
    fetch('/api/manifest')
      .then((res) => res.json())
      .then((data: Manifest) => {
        setProjectTitle(data.name);
        if (data.sources) {
          setSources(data.sources);
        }
      })
      .catch(() => setProjectTitle('(could not load project)'));
  }, []);

  // Fetch edit names, pick the first
  useEffect(() => {
    fetch('/api/edits')
      .then((res) => res.json())
      .then((names: string[]) => {
        setEditNames(names);
        if (names.length > 0 && !editName) {
          setEditName(names[0]);
        }
      })
      .catch(() => {});
  }, []);

  // Fetch edit document when editName changes
  useEffect(() => {
    if (!editName) return;
    fetch(`/api/edits/${editName}`)
      .then((res) => res.json())
      .then((data: EditDocumentRaw) => {
        const shots = (data.snapshot?.shots ?? []).map(normaliseShot);
        const doc: EditDocument = { name: data.name, shots };
        setEditDocument(doc);
        if (!selectedShotId && shots.length > 0) {
          setSelectedShotId(shots[0].id);
        }
      })
      .catch(() => {});
  }, [editName]);

  // Fetch transcript when a shot is selected
  useEffect(() => {
    if (!selectedShotId || !editDocument) {
      setTranscript(null);
      return;
    }
    const shot = editDocument.shots.find((s) => s.id === selectedShotId);
    if (!shot) {
      setTranscript(null);
      return;
    }
    fetch(`/api/transcripts/${shot.source_id}`)
      .then((res) => {
        if (!res.ok) throw new Error('not found');
        return res.json();
      })
      .then((data: Transcript) => setTranscript(data))
      .catch(() => setTranscript(null));
  }, [selectedShotId, editDocument]);

  const selectShot = useCallback((id: string | null) => {
    setSelectedShotId(id);
  }, []);

  const selectedSource = editDocument && selectedShotId
    ? sources.find((s) => s.id === editDocument.shots.find((sh) => sh.id === selectedShotId)?.source_id)
    : undefined;

  const reorderShot = useCallback((fromIndex: number, toIndex: number) => {
    setEditDocument((prev) => {
      if (!prev) return prev;
      const shots = [...prev.shots];
      const [moved] = shots.splice(fromIndex, 1);
      shots.splice(toIndex, 0, moved);
      return { ...prev, shots };
    });
  }, []);

  const trimShot = useCallback((shotId: string, newRange: ShotRange) => {
    setEditDocument((prev) => {
      if (!prev) return prev;
      const shots = prev.shots.map((s) =>
        s.id === shotId ? { ...s, range: newRange } : s,
      );
      return { ...prev, shots };
    });
  }, []);

  const splitShot = useCallback((shotId: string, splitMs: number) => {
    setEditDocument((prev) => {
      if (!prev) return prev;
      const idx = prev.shots.findIndex((s) => s.id === shotId);
      if (idx === -1) return prev;
      const shot = prev.shots[idx];
      const r = shot.range;

      let leftRange: ShotRange;
      let rightRange: ShotRange;

      switch (r.type) {
        case 'time': {
          if (splitMs <= r.from_ms || splitMs >= r.to_ms) return prev;
          leftRange = { type: 'time', from_ms: r.from_ms, to_ms: splitMs };
          rightRange = { type: 'time', from_ms: splitMs, to_ms: r.to_ms };
          break;
        }
        case 'words': {
          // splitMs is treated as a word index for word-based ranges
          const splitWord = Math.round(splitMs);
          if (splitWord <= r.from || splitWord > r.to) return prev;
          leftRange = { type: 'words', from: r.from, to: splitWord - 1 };
          rightRange = { type: 'words', from: splitWord, to: r.to };
          break;
        }
        case 'scenes': {
          const splitScene = Math.round(splitMs);
          if (splitScene <= r.from || splitScene > r.to) return prev;
          leftRange = { type: 'scenes', from: r.from, to: splitScene - 1 };
          rightRange = { type: 'scenes', from: splitScene, to: r.to };
          break;
        }
      }

      const leftShot = { ...shot, id: `${shot.id}-a`, range: leftRange };
      const rightShot = { ...shot, id: `${shot.id}-b`, range: rightRange };

      const shots = [...prev.shots];
      shots.splice(idx, 1, leftShot, rightShot);
      return { ...prev, shots };
    });
  }, []);

  const deleteShot = useCallback((shotId: string) => {
    setEditDocument((prev) => {
      if (!prev) return prev;
      const shots = prev.shots.filter((s) => s.id !== shotId);
      return { ...prev, shots };
    });
    setSelectedShotId((prev) => (prev === shotId ? null : prev));
  }, []);

  return {
    editDocument,
    editNames,
    selectedShotId,
    selectShot,
    sources,
    selectedSource,
    transcript,
    reorderShot,
    trimShot,
    splitShot,
    deleteShot,
    projectTitle,
  };
}
