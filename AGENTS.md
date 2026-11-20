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

The primary objective is maintaining rock-solid app stability, ultra-low power consumption, and hardware decode reliability on Wayland / Linux (AMD VA-API DMA-BUF, GStreamer, and NVIDIA RTD3 isolation).

Before presenting any changes for commit approval:
1. **Build Integrity**:
   - Ensure the project builds cleanly without warnings or errors (`cargo check --all-targets`, `cargo test --release`, and `./build.sh`).
2. **Desktop & Shell Integration**:
   - If desktop entries, scripts, or assets are touched, validate them (`desktop-file-validate`, `bash -n`).
   - Ensure install scripts remain native-default, turnkey, and industrial-grade (standalone binaries via `install -p -m 755`, **zero fragile symlinks**, zero container requirement by default).
3. **Hardware & Subsystem Verification**:
   - Verify hardware isolation probe (`./target/release/video-player --probe`).
   - Verify hardware decode, DMA-BUF zero-copy, and UI stability (`./target/release/video-player --test-stress`).
4. **Clean Tree**:
   - Ensure no temporary files (`*.swp`, `.cache`, ad-hoc build artifacts) pollute the working tree.

---

## 3. Architecture & Maximum Performance Protocol

1. **SOLID 3-Layer Segregation (Brother Architecture to Photos)**:
   - `src/core/`: Pure, hardware-agnostic & toolkit-agnostic logic only (zero GTK/GStreamer/OS imports).
   - `src/hw/`: All Linux/DRM/PCI/sysfs/hwmon/GStreamer hardware isolation and telemetry code.
   - `src/ui/`: GTK 4.24 / Libadwaita presentation and 0 Hz event-driven idle loop.
2. **Native Zen 3 Machine Code**:
   - `.cargo/config.toml` must enforce `-C target-cpu=znver3`.
   - `Cargo.toml` must enforce `lto = "fat"`, `codegen-units = 1`, `strip = "symbols"`, and `panic = "abort"`.
   - Optional two-pass PGO is supported via `./build.sh --pgo [VIDEO_OR_DIR]` / `./install.sh --pgo [VIDEO_OR_DIR]` (using `--test-stress` and brief 2s local samples in isolated temp XDG dirs).

---

## 4. Communication Standards

- Be concise and provide direct file links (`file:///...`) to modified files.
- Summarize changes before requesting approval to commit.
- Never assume implicit permission for VCS mutations.
