# Developer Mode (Windows)

A Windows setting (Settings → For developers) that, among other things, grants
the current user the `SeCreateSymbolicLinkPrivilege` — allowing an **ordinary,
non-elevated** process to create [symbolic links]. Without it, symbolic-link
creation on Windows requires running elevated (as administrator), which a
typical end user will not do for a CLI tool.

This is the pivot in [[ADR-012-cross-platform-source-linking]]: `ar-edit` cannot
assume symbolic links are creatable on Windows. When Developer Mode is enabled
(or the process is elevated) it uses symlinks for parity with macOS/Linux;
otherwise it falls back to a [[hard link]] or a copy, so source registration
([[SPEC-001-transcript-video-editor#REQ-002]]) never fails for lack of
privilege.
