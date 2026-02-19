---
marp: true
theme: default
paginate: true
backgroundColor: #fff
color: #222
style: |
  section {
    font-family: 'Helvetica Neue', Arial, sans-serif;
  }
  h1 {
    color: #111;
  }
  h2 {
    color: #222;
  }
  strong {
    color: #111;
  }
  em {
    color: #666;
  }
  code {
    background: #f0f0f0;
    color: #333;
    padding: 2px 6px;
    border-radius: 4px;
  }
  pre {
    background: #f5f5f5;
    border: 1px solid #ddd;
    border-radius: 8px;
  }
  pre code {
    background: transparent;
    color: #333;
  }
  table {
    font-size: 0.85em;
  }
  th {
    background: #f0f0f0;
    color: #222;
  }
  td {
    border-color: #ddd;
  }
  blockquote {
    border-left: 4px solid #333;
    color: #555;
    font-style: italic;
  }
  a {
    color: #0066cc;
  }
  section::after {
    color: #999;
  }
---

# ar-edit

### A tool for LLM agent driven video editing

---

## The problem

You have **3 interview clips** and a pile of **B-roll**.

You need a **5-minute cut**.

The traditional workflow:

1. Import into an NLE
2. Scrub through hours of timeline
3. Find the good bits by listening
4. Manually mark in/out points
5. Arrange on a timeline
6. Repeat until done

**Most of the time is spent _finding_, not _editing_.**

---

## What if you could edit the text instead?

Imagine your videos are automatically **transcribed** with word-level timestamps.

You open the transcript in a text editor.

You **delete the bad parts**.
You **rearrange the good parts**.
You **copy a paragraph from interview B** and paste it after interview A.

Hit render.

**The video follows the text.**

---

## That's ar-edit.

```
                     ┌──────────────┐
   Video files ────► │   Analyse    │ ────► Transcripts + scene descriptions
                     └──────────────┘
                            │
                            ▼
                     ┌──────────────┐
                     │  Edit text   │ ────► Edit document (shot list)
                     └──────────────┘
                            │
                            ▼
                     ┌──────────────┐
                     │   Render     │ ────► Final video
                     └──────────────┘
```

---

## Understanding speech

whisper.cpp transcribes locally, word by word.

Each word gets a precise timestamp:

| Word | Start | End |
|------|-------|-----|
| Welcome | 0.00s | 0.42s |
| to | 0.42s | 0.54s |
| the | 0.54s | 0.65s |
| interview | 0.65s | 1.12s |
| today | 1.12s | 1.45s |

You never type a timecode.
**Words _are_ the timecodes.**

---

## Understanding vision

Thumbnails are extracted at scene changes and described by an LLM:

| Scene | Timestamp | Description |
|-------|-----------|-------------|
| scene-0 | 0:00 | *Wide shot, two people at desk, city skyline through window* |
| scene-1 | 0:18 | *Close-up of speaker, bookshelf background* |
| scene-2 | 0:45 | *Cutaway to documents on table* |

Every video gets **both layers**.

An interview has words AND visual context.
B-roll has scenes AND maybe ambient audio.

---

## Multi-source editing

You have 3 interviews: **Alice**, **Bob**, and **Carol**.

Your edit is just a list of shots:

| # | Source | What they're saying |
|---|--------|-------------------|
| shot-001 | Alice | *"The impact on coastal communities has been..."* |
| shot-002 | Carol | *"What we found in the regional assessment..."* |
| shot-003 | Bob | *"And that's exactly why the policy changed..."* |
| shot-004 | Alice | *"Looking forward, I think the key challenge..."* |

Each shot points at a **word range** in the transcript.
ar-edit resolves the timestamps and cuts the video.

---

## Mixing speech and B-roll

Not all footage has speech. Drone shots. Time-lapses. Cutaways.

You can mix **speech clips** and **visual clips** in the same edit:

| # | Source | Content |
|---|--------|---------|
| shot-001 | Alice | *"The impact on coastal communities..."* |
| shot-002 | Drone | *Aerial coastline at sunset, waves on reef* |
| shot-003 | Bob | *"And that's exactly why..."* |
| shot-004 | Drone | *Close-up of coral formation underwater* |

The agent sees both transcripts and scene descriptions to make these choices.

---

## The AI angle

An **LLM agent** can drive the whole tool. You say:

> "Make a 2-minute highlight reel focusing on climate policy. Use the coastline B-roll between speakers."

The agent reads your transcripts and scene descriptions, picks the best segments, assembles the edit, and renders the output.

**You review. The agent revises.**

---

## Marking your selects

Before the agent edits, you review the raw footage and **mark the good parts**:

| Marker | Source | Label | Note |
|--------|--------|-------|------|
| mark-001 | Alice | **select** | *Best take of the climate answer* |
| mark-002 | Alice | avoid | *Audio spike, unusable* |
| mark-003 | Bob | **hero** | *Great closing statement* |
| mark-004 | Drone | select | *Best coastline pass* |

The agent reads your markers and builds the edit from your **selects** and **heroes**, skipping anything marked **avoid**.

After watching the assembled edit, you add **shot notes** — "shot-003 is too long", "great energy in shot-005". The agent reads those notes for the next revision.

---

## The feedback loop

You watch the edit in **VLC** with an overlay:

```
┌─────────────────────────────────────────────┐
│ 00:01:23.450  shot-005  src-002             │
│                                             │
│                                             │
│           [ video playback ]                │
│                                             │
│                                             │
└─────────────────────────────────────────────┘
```

You pause. You see **shot-005** from **src-002** at **1:23**.

You tell the agent:

> "On shot-005, cut 3 seconds earlier. And swap shot-003 and shot-004."

The agent adjusts. You watch again.

---

## Non-destructive, always

**Nothing is ever lost.**

| What | Protected how? |
|------|---------------|
| Source videos | Never touched. Read-only. |
| Transcripts | Immutable. Regenerate, don't modify. |
| Scene descriptions | Append-only. |
| Your edits | **Full undo history.** Every change recorded. |

Every edit operation is logged. `ctrl-z` walks backwards through history.

Made 20 changes and want to go back to version 3? You can.

The agent made a bad cut? Undo it. The data for the deleted shot is **still there**.

---

## Under the hood: project structure

```
project/
├── manifest.json            # Sources, settings
├── sources/
│   ├── src-001.mp4          # Interview Alice
│   ├── src-002.mp4          # Drone B-roll
│   └── src-003.mp4          # Interview Bob
├── transcripts/
│   ├── src-001.transcript.json
│   └── src-003.transcript.json
├── index/
│   ├── src-001.index.json   # Scenes + descriptions
│   └── src-002.index.json
├── thumbnails/
│   ├── src-001_00m00s.jpg
│   ├── src-002_00m10s.jpg
│   └── ...
└── edits/
    └── rough-cut.edit.json  # Your edit — just pointers
```

---

## Under the hood: the edit document

An edit is a list of shots. Each shot is just a **pointer** into a transcript or scene index:

```json
{
  "shots": [
    { "id": "shot-001", "source": "src-001",
      "range": { "words": { "from": 0, "to": 52 } } },

    { "id": "shot-002", "source": "src-002",
      "range": { "scenes": { "from": 0, "to": 2 } } },

    { "id": "shot-003", "source": "src-003",
      "range": { "words": { "from": 10, "to": 90 } } }
  ]
}
```

**No video data. No timestamps.** Timing is looked up from the transcript at render time. Re-transcribe with a better model? Edits still work.

---

## Under the hood: how it connects

```
  transcript (what's said)       index (what's shown)
   ┌──────────────────┐          ┌──────────────────┐
   │ word 0: "Welcome"│          │ scene 0: "Wide   │
   │   0ms – 420ms    │          │   shot, office"  │
   │ word 1: "to"     │          │   0ms – 18000ms  │
   │   420ms – 540ms  │          │ scene 1: "Close  │
   │ ...               │          │   up, speaker"   │
   │ word 52: "about" │          │   18000ms– 45000ms│
   │   12100ms–12400ms│          └──────────────────┘
   └──────────────────┘                  ▲
            ▲                            │
            │           edit document    │
            │          ┌─────────────┐   │
            └──────────│ shot-001    │   │
                       │  words 0–52 │   │
                       │ shot-002    │───┘
                       │  scenes 0–2 │
                       └─────────────┘
```

The edit is a **join table**. It references both layers. A 100-shot edit is **2 kilobytes**.

---

## Interactive terminal UI

For hands-on editing, ar-edit has a **terminal interface**:

```
┌─ Sources ──────┬─ Timeline ──────────────────────────────┐
│                │                                         │
│ ► src-001 ✓✓  │  shot-001  src-001  12.4s  "Welcome..." │
│   src-002 ✓✓  │ ►shot-002  src-003  14.8s  "What we..." │
│   src-003 ✓─  │  shot-003  src-002   7.0s  [coastline]  │
│                │  shot-004  src-001   9.8s  "Looking..."  │
├────────────────┼─────────────────────────────────────────┤
│ ✓ transcribed  │ Transcript: src-003                     │
│ ✓ indexed      │                                         │
│                │ ...the regional assessment was that     │
│                │ ████████████████████████████████████     │
│                │ communities have been significantly     │
│                │ ████████████████████████████████████     │
│                │ affected by the changes in...           │
│                │                                         │
├────────────────┴─────────────────────────────────────────┤
│ ► Playing shot-002 | 00:00:23.4 | src-003                │
└──────────────────────────────────────────────────────────┘
```

*Navigate with keyboard. Play with Enter. Undo with ctrl-z.*

---

## Search across everything

```
> search "coastal communities"
```

| Source | Type | Timestamp | Match |
|--------|------|-----------|-------|
| src-001 | transcript | 0:45 | *"...impact on **coastal communities** has been..."* |
| src-003 | transcript | 1:12 | *"...**coastal communities** are adapting..."* |
| src-002 | scene | 0:18 | *"Aerial shot of **coastal** town from above"* |

Search finds matches in **transcripts** and **visual descriptions**.

Found what you want? Add it to the edit in one keystroke.

---

## How it all fits together

```
     You                          AI Agent
      │                              │
      │   "Focus on climate,         │
      │    use the drone shots"      │
      │ ────────────────────────►    │
      │                              │ reads transcripts
      │                              │ reads scene descriptions
      │                              │ assembles edit
      │    ◄──────────────────────── │
      │   renders preview            │
      │                              │
      │   watches in VLC             │
      │   "shot-005 cut earlier,     │
      │    swap 3 and 4"             │
      │ ────────────────────────►    │
      │                              │ adjusts edit
      │    ◄──────────────────────── │
      │   watches again              │
      │   "looks good, render"       │
      │ ────────────────────────►    │
      │                              │ final render
      │    ◄──────────────────────── │
      │   final.mp4                  │
```

---

## What it's built on

All open-source, all local. No cloud.

| Component | What it does |
|-----------|-------------|
| **ffmpeg** | Video processing, rendering, thumbnails |
| **whisper.cpp** | Local speech-to-text transcription |
| **LLM** | Scene description from thumbnails |
| **VLC** | Video playback with seeking |
| **Rust** | The ar-edit tool itself |

Everything runs on your machine.
Your footage never leaves your disk.

---

## What's different from Descript?

| | Descript | ar-edit |
|---|---------|---------|
| Open source | No | **Yes** |
| Runs locally | Cloud-dependent | **Fully local** |
| AI editing | Built-in, limited | **Any LLM agent** |
| B-roll support | Timeline only | **LLM-described scenes** |
| Undo model | Standard | **Full operation history** |
| Programmable | No | **CLI + JSON for automation** |
| Cost | $24/mo+ | **Free** |
| Multi-source | Yes | **Yes, with cross-source search** |

---

## In one sentence

**ar-edit lets you and an AI agent edit video by reading and rearranging text, not scrubbing timelines.**

---

# Want to try it?

It's early days. The spec is written. The build is starting.

*Feedback welcome.*
