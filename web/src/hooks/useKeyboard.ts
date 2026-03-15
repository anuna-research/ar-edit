import { useEffect } from 'react';
import type { EditDocument } from '../types';

export interface PlayTarget {
  edit: string;
  shot: string;
}

async function playShot(target: PlayTarget): Promise<void> {
  try {
    await fetch('/api/play', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(target),
    });
  } catch {
    // fire-and-forget: swallow network errors
  }
}

export interface UseKeyboardOptions {
  editDocument: EditDocument | null;
  selectedShotId: string | null;
  selectShot: (id: string | null) => void;
  onTogglePlay?: () => void;
}

export function useKeyboard({
  editDocument,
  selectedShotId,
  selectShot,
  onTogglePlay,
}: UseKeyboardOptions): void {
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      // Ignore when typing in an input/textarea
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;

      const shots = editDocument?.shots ?? [];
      const currentIndex = shots.findIndex((s) => s.id === selectedShotId);

      switch (e.key) {
        case 'p': {
          if (!editDocument || !selectedShotId) return;
          playShot({ edit: editDocument.name, shot: selectedShotId });
          break;
        }
        case 'j': {
          // Next shot
          if (shots.length === 0) return;
          const nextIndex = currentIndex < 0 ? 0 : Math.min(currentIndex + 1, shots.length - 1);
          selectShot(shots[nextIndex].id);
          break;
        }
        case 'k': {
          // Previous shot
          if (shots.length === 0) return;
          const prevIndex = currentIndex < 0 ? 0 : Math.max(currentIndex - 1, 0);
          selectShot(shots[prevIndex].id);
          break;
        }
        case ' ': {
          // Space = play/pause
          onTogglePlay?.();
          break;
        }
        case 'Escape': {
          selectShot(null);
          break;
        }
        default:
          return; // Don't preventDefault for unhandled keys
      }

      e.preventDefault();
    }

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [editDocument, selectedShotId, selectShot, onTogglePlay]);
}
