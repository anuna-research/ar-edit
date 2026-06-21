# Hard link

A second directory entry that points at the **same underlying file data** (the
same inode / file record) as an existing file. Unlike a symbolic link, a hard
link is not a path-bearing pointer to another path — it is an equal name for the
same bytes, so it does not require the target to be reachable by path and does
not need the elevated privilege that symbolic-link creation does on Windows.

Constraints: a hard link must live on the **same volume** as the data it shares,
and it links files, not directories.

In [[ADR-012-cross-platform-source-linking]] a hard link is the preferred
Windows fallback when symbolic links are unavailable (no [[Developer Mode]], not
elevated) and the source file is on the same volume as the project: it places a
source under `sources/` with **no byte duplication** and no privilege
requirement, transparently to ffmpeg/whisper/VLC. If the file is on another
volume, the system falls back to a full copy.
