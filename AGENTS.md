# Agent Guidelines & Development Protocol

This document defines mandatory operating rules for all AI agents (Antigravity, Gemini, Claude, Cursor, Copilot, etc.) working on the **Video Player** repository.

---

## 1. Mandatory User Confirmation Before Git Actions

### 1.1 Do NOT Commit Without Explicit User Approval
- **Rule**: Agents must **NEVER** run `git commit` autonomously.
- **Protocol**:
  1. Complete code edits and internal verification.
  2. Display a clear summary of changes and the `git diff`.
  3. Propose a descriptive Conventional Commit message (e.g., `feat(...)`, `fix(...)`, `chore(...)`).
  4. Explicitly ask the user for confirmation before executing the commit.

### 1.2 Do NOT Push Without Explicit User Approval
- **Rule**: Agents must **NEVER** run `git push` to any remote repository without explicit user authorization.
- **Protocol**:
  1. Confirm that all commits are fully verified and approved.
  2. Inform the user of the target remote, branch, and tags to be pushed.
  3. Wait for direct user consent before running `git push`.

---

## 2. Stability First: 100% Rigorous Verification

The primary objective is maintaining rock-solid app stability, high performance, and hardware decode reliability on Wayland / Linux (AMD VA-API DMA-BUF, GStreamer, and NVIDIA RTD3 isolation).

Before presenting any changes for commit approval:
1. **Build Integrity**:
   - Ensure the project builds cleanly without warnings or errors (`cargo check` or `./build.sh --native` / container build).
2. **Desktop & Shell Integration**:
   - If desktop entries, scripts, or assets are touched, validate them (`desktop-file-validate`).
   - Ensure install scripts remain turnkey and industrial-grade (standalone binaries via `install -p -m 755`, **zero fragile symlinks**).
3. **Hardware & Subsystem Verification**:
   - Verify hardware decode and pipeline stability (`./target/release/video-player --test-stress`).
   - Verify telemetry and GPU isolation logic (`./target/release/video-player --hud`).
4. **Clean Tree**:
   - Ensure no temporary files (`*.swp`, `*.tmp_rot`, ad-hoc build artifacts) pollute the working tree.

---

## 3. Communication Standards

- Be concise and provide direct file links (`file:///...`) to modified files.
- Summarize changes before requesting approval to commit.
- Never assume implicit permission for VCS mutations.
