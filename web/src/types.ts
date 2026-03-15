export interface Manifest {
  name: string;
  version: string;
  created: string;
  sources: Source[];
}

export interface Source {
  id: string;
  path: string;
  original_filename: string;
  added: string;
  duration_ms?: number;
  video_codec?: string;
  audio_codec?: string;
  resolution?: [number, number];
  frame_rate?: number;
  transcribed?: boolean;
  indexed?: boolean;
}

// Raw API response — event-sourced edit document
export interface EditDocumentRaw {
  name: string;
  created: string;
  snapshot?: { shots: ShotRaw[] };
  ops: unknown[];
}

// Normalised for the UI
export interface EditDocument {
  name: string;
  shots: Shot[];
}

// Raw shot from API: range is a tagged union via key, source is "source" not "source_id"
export interface ShotRaw {
  id: string;
  source: string;
  range: RangeRaw;
  note?: string;
}

export type RangeRaw =
  | { words: { from: number; to: number } }
  | { scenes: { from: number; to: number } }
  | { time: { from_ms: number; to_ms: number } };

export interface Shot {
  id: string;
  source_id: string;
  range: ShotRange;
  note?: string;
}

export type ShotRange =
  | { type: 'words'; from: number; to: number }
  | { type: 'scenes'; from: number; to: number }
  | { type: 'time'; from_ms: number; to_ms: number };

// Convert raw API shot to normalised Shot
export function normaliseShot(raw: ShotRaw): Shot {
  let range: ShotRange;
  if ('words' in raw.range) {
    range = { type: 'words', from: raw.range.words.from, to: raw.range.words.to };
  } else if ('scenes' in raw.range) {
    range = { type: 'scenes', from: raw.range.scenes.from, to: raw.range.scenes.to };
  } else {
    range = { type: 'time', from_ms: raw.range.time.from_ms, to_ms: raw.range.time.to_ms };
  }
  return { id: raw.id, source_id: raw.source, range, note: raw.note };
}

export interface Transcript {
  source_id: string;
  model?: string;
  language?: string;
  duration_ms?: number;
  segments: Segment[];
  word_count?: number;
}

export interface Segment {
  index: number;
  start_ms: number;
  end_ms: number;
  text: string;
  words: Word[];
}

export interface Word {
  index: number;
  text: string;
  start_ms: number;
  end_ms: number;
  confidence?: number;
}

// Flatten all words from a transcript
export function flattenWords(transcript: Transcript): Word[] {
  return transcript.segments.flatMap((seg) => seg.words);
}
