# ConPTY

The **Console Pseudo Terminal** API introduced in Windows 10 (1809). It gives
Windows a real pseudo-terminal abstraction — analogous to Unix PTYs — so that
terminal applications can do cursor movement, colour, raw input, and resize
handling through a consistent interface rather than the legacy Win32 console
API.

It matters here because the `ar-edit` TUI is built on ratatui + crossterm, and
**crossterm uses ConPTY as its Windows backend**. ConPTY is why the TUI
([[SPEC-001-transcript-video-editor#REQ-038]]) can run on Windows with the same
rendering and key-handling code as macOS/Linux, and why
[[SPEC-001-transcript-video-editor#NFR-015]] can claim an identical command and
interaction surface across platforms. It works in modern hosts (Windows
Terminal, recent `conhost.exe`); very old `cmd.exe`/console hosts predating
ConPTY are out of the supported matrix.
