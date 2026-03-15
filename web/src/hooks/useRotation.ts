import { useState, useCallback, useEffect } from 'react';
import type { Source } from '../types';

export interface UseRotationReturn {
  rotations: Map<string, number>;
  getRotation: (sourceId: string) => number;
  cycleRotation: (sourceId: string) => void;
}

export function useRotation(sources: Source[]): UseRotationReturn {
  const [rotations, setRotations] = useState<Map<string, number>>(new Map());

  // Fetch rotation for each source on load
  useEffect(() => {
    for (const source of sources) {
      fetch(`/api/sources/${encodeURIComponent(source.id)}/rotation`)
        .then((res) => res.json())
        .then((data: { degrees: number }) => {
          setRotations((prev) => {
            if (prev.get(source.id) === data.degrees) return prev;
            const next = new Map(prev);
            next.set(source.id, data.degrees);
            return next;
          });
        })
        .catch(() => {});
    }
  }, [sources]);

  const getRotation = useCallback(
    (sourceId: string): number => {
      return rotations.get(sourceId) ?? 0;
    },
    [rotations],
  );

  const cycleRotation = useCallback(
    (sourceId: string) => {
      const current = rotations.get(sourceId) ?? 0;
      const next = (current + 90) % 360;
      // Optimistic update
      setRotations((prev) => {
        const m = new Map(prev);
        m.set(sourceId, next);
        return m;
      });
      // Persist to backend
      fetch(`/api/sources/${encodeURIComponent(sourceId)}/rotation`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ degrees: next }),
      }).catch(() => {
        // Revert on failure
        setRotations((prev) => {
          const m = new Map(prev);
          m.set(sourceId, current);
          return m;
        });
      });
    },
    [rotations],
  );

  return { rotations, getRotation, cycleRotation };
}
