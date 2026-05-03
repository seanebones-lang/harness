# Harness — installation guide

This guide walks through installing Harness on every OS the project **tests in CI** and supports in the field: **macOS**, **Linux**, and **Windows** (native and **WSL2**). Optional features differ by platform; see **Optional features** at the end.

**Quick links:** [macOS](#macos) · [Linux](#linux) · [Windows](#windows-native) · [WSL2](#windows-subsystem-for-linux-wsl2) · [After installing](#after-installing) · [Updating](#updating) · [Uninstall](#uninstall)

---

## Supported platforms

| Environment | Status | CI |
|-------------|--------|-----|
| **macOS** (Apple Silicon and Intel) | Fully supported | `macos-latest` |
| **Linux** (typical glibc distros, e.g. Ubuntu, Fedora) | Fully supported | `ubuntu-latest` |
| **Windows** 10 / 11 (native PowerShell) | Fully supported | `windows-latest` |
| **WSL2** (Linux distro on Windows) | Treat as **Linux** | Same as Linux |

Harness does not ship OS-specific installers (`.msi`, `.dmg`, `.deb`). You **build from source** with Rust or use **GitHub Releases** binaries when published (see [README](../README.md)).

---

## Requirements (all platforms)

1. **Rust** — stable toolchain, edition 2021. Install with [rustup](https://rustup.rs).  
   - **Windows:** choose the **x86_64-pc-windows-msvc** default (MSVC). If the installer offers GNU vs MSVC, prefer **MSVC** unless you know you need GNU.

2. **Git** — to clone the repository.  
   - **Windows:** [Git for Windows](https://git-scm.com/download/win) is strongly recommended (it puts `git` and often `bash`/`sh` on `PATH`, which improves the **`shell` tool**).

3. **An LLM backend** — at least one of:
   - `ANTHROPIC_API_KEY` (recommended default path in config), or  
   - `XAI_API_KEY`, or  
   - `OPENAI_API_KEY`, or  
   - Local **[Ollama](https://ollama.com)** with a chat model (no cloud key).

4. **Optional — semantic memory embeddings** — default config uses `nomic-embed-text`. That usually means **Ollama** running locally (`ollama pull nomic-embed-text`) unless you change `[memory].embed_model` in config.

---

## macOS

### 1. Install Rust and Git

```bash
# Rust (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Xcode command line tools (gcc/clang for some crates; often already installed)
xcode-select --install   # skip if already installed
```

### 2. Choose an install method

**Option A — install script (from any directory)**

Review scripts before piping to a shell. From a temporary directory:

```bash
curl -fsSL https://raw.githubusercontent.com/seanebones-lang/harness/main/scripts/install.sh | bash
```

The script clones the repo into a temp dir, runs `cargo build --profile release-lto`, installs `harness` to `~/.local/bin`, and creates `~/.harness/config.toml` if missing.

Custom install location:

```bash
export HARNESS_INSTALL_DIR="$HOME/bin"
curl -fsSL https://raw.githubusercontent.com/seanebones-lang/harness/main/scripts/install.sh | bash
```

**Option B — clone and build yourself**

```bash
git clone https://github.com/seanebones-lang/harness.git
cd harness
cargo build --profile release-lto
install -m 755 target/release-lto/harness ~/.local/bin/harness
```

If `release-lto` is missing, use `target/release/harness` after `cargo build --release`.

### 3. Put the binary on `PATH`

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc   # or ~/.bashrc
source ~/.zshrc
```

### 4. Verify

```bash
harness --version
harness --help
harness doctor
```

### macOS — FAQ

| Question | Answer |
|----------|--------|
| Apple Silicon vs Intel? | Same steps; rustup installs the correct architecture. |
| Do I need Homebrew? | No. Brew is optional for tools like `ollama`, `gh`, `cliclick`. |
| Where is config? | `~/.harness/config.toml`. Project overrides: `.harness/config.toml`. |
| Notifications don’t show | Grant Terminal (or iTerm) notification permission in **System Settings → Notifications**. |

### macOS — troubleshooting

| Problem | What to try |
|---------|-------------|
| `command not found: harness` | Confirm `~/.local/bin` is on `PATH` and open a **new** terminal. Run `which harness`. |
| `cargo: command not found` | Run `source "$HOME/.cargo/env"` or restart the shell after rustup. |
| Linker errors when building | Run `xcode-select --install`. Ensure CLT finished installing. |
| Build very slow first time | Normal; Rust is compiling the dependency graph. Subsequent builds are faster. |
| `harness` exits: no API key | Export `ANTHROPIC_API_KEY` (or another key), or run Ollama locally and configure the router for `ollama`. |

---

## Linux

### 1. OS packages (recommended before `cargo build`)

**Debian / Ubuntu**

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev git curl
```

**Fedora / RHEL / Rocky**

```bash
sudo dnf install -y gcc gcc-c++ pkgconf openssl-devel git curl
```

**Arch**

```bash
sudo pacman -S --needed base-devel openssl pkgconf git curl
```

Some minimal images omit SSL dev headers; without them, crates using native TLS may fail to link until `libssl-dev` / `openssl-devel` is installed.

### 2. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

### 3. Install Harness

**Option A — script**

```bash
curl -fsSL https://raw.githubusercontent.com/seanebones-lang/harness/main/scripts/install.sh | bash
```

**Option B — manual clone**

```bash
git clone https://github.com/seanebones-lang/harness.git
cd harness
cargo build --profile release-lto
mkdir -p ~/.local/bin
cp target/release-lto/harness ~/.local/bin/
chmod +x ~/.local/bin/harness
```

### 4. `PATH`

Add to `~/.bashrc`, `~/.zshrc`, or equivalent:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

### 5. Verify

```bash
harness --version
harness doctor
```

### Linux — FAQ

| Question | Answer |
|----------|--------|
| Which distros work? | Glibc-based desktop/server distros are the usual target. CI uses Ubuntu. |
| Wayland vs X11? | Core TUI works on both. Some extras (**notifications**, **xdotool** for computer use) depend on your session. |
| Flatpak/Snap sandbox? | Building inside a dev container is fine; ensure network access for crates.io and your API. |

### Linux — troubleshooting

| Problem | What to try |
|---------|-------------|
| `failed to run custom build command` / OpenSSL | Install `libssl-dev` (Debian) or `openssl-devel` (Fedora). |
| `linker cc not found` | Install `build-essential` or `gcc`. |
| `harness: not found` | Fix `PATH`; log out and back in if you edited shell rc files. |
| SQLite errors | The workspace uses SQLite with bundled libsql; persistent errors often mean **disk full** or **permissions** on `~/.harness`. |
| Desktop notifications missing | Install **`libnotify`** / `libnotify-bin` and a running notification daemon (varies by DE). |

---

## Windows (native)

Harness is developed and tested with **PowerShell** and the **MSVC** toolchain.

### 1. Install prerequisites

1. **Rust** — https://rustup.rs — default **MSVC** host triple.  
2. **Visual Studio Build Tools** — if rustup prompts for C++ build tools, install **“Desktop development with C++”** workload (or the minimal MSVC + Windows SDK components rustup links to).  
3. **Git for Windows** — https://git-scm.com/download/win — use the option to add Git to PATH.

### 2. Install Harness

**Option A — script (from PowerShell)**

Prefer running from a **cloned repo** so you can inspect the script:

```powershell
cd C:\path\to\harness   # repo root, contains Cargo.toml
Set-ExecutionPolicy -Scope CurrentUser RemoteSigned   # if scripts are blocked
.\scripts\install.ps1
```

To install from the network (review the script in a browser first):

```powershell
Invoke-RestMethod https://raw.githubusercontent.com/seanebones-lang/harness/main/scripts/install.ps1 | Invoke-Expression
```

**Option B — manual**

```powershell
git clone https://github.com/seanebones-lang/harness.git
cd harness
cargo build --profile release-lto
New-Item -ItemType Directory -Force -Path "$HOME\.local\bin" | Out-Null
Copy-Item -Force .\target\release-lto\harness.exe "$HOME\.local\bin\harness.exe"
```

### 3. Add `harness` to PATH

1. Windows **Settings → System → About → Advanced system settings → Environment Variables**.  
2. Under **User** variables, edit **Path**, add:

   `%USERPROFILE%\.local\bin`

3. **Close and reopen** PowerShell or Terminal.

### 4. API keys in PowerShell (session only)

```powershell
$env:ANTHROPIC_API_KEY = "sk-ant-..."
harness
```

Persist via **User environment variables** in the same UI, or a profile script.

### 5. Verify

```powershell
harness --version
harness doctor
```

### Windows — FAQ

| Question | Answer |
|----------|--------|
| PowerShell vs Command Prompt? | Use **PowerShell** for the install script; `harness.exe` runs from either once on PATH. |
| Why Git for Windows? | The **`shell` tool** behaves best when `sh`/`bash` from Git is on PATH; otherwise Harness may fall back to **`cmd.exe /C`**, which is not POSIX. |
| VS Code extension / daemon socket? | The optional VS Code integration expects a **Unix domain socket** under the user profile; native Windows is awkward for that. Prefer **WSL2** or Linux/macOS for extension + daemon workflows (see [README](../README.md) “Optional features”). |
| Computer use / `cliclick`? | **Not supported** on native Windows the same way as macOS/Linux. |

### Windows — troubleshooting

| Problem | What to try |
|---------|-------------|
| `cargo` not recognized | Reopen terminal after rustup; or `$env:Path += ";$HOME\.cargo\bin"`. |
| MSVC / linker errors (`link.exe` missing) | Install VS Build Tools with **C++** workload; run `rustup default stable-msvc`. |
| `running scripts is disabled` | `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`. |
| `harness` not found | Confirm `%USERPROFILE%\.local\bin` on **User** PATH and open a **new** terminal. |
| `shell` tool behaves oddly | Add Git’s `usr\bin` to PATH ahead of system32; use WSL2 for a real Linux shell. |
| Antivirus blocks build | Exclude the repo `target\` folder temporarily or allow `rustc`/`cargo`. |

---

## Windows Subsystem for Linux (WSL2)

If you already use **WSL2**, install Harness **inside Ubuntu (or another WSL distro)** using the **Linux** section above. Benefits:

- POSIX **`shell` tool** without extra Windows quirks.  
- Closer match to CI and many team dev environments.  
- Easier path for tooling that expects Unix sockets (daemon, some editor integrations).

**Install WSL (once, from elevated PowerShell):**

```powershell
wsl --install
```

Reboot if prompted; launch **Ubuntu** from Start, create a user, then follow [Linux](#linux) inside that terminal.

### WSL — FAQ / troubleshooting

| Problem | What to try |
|---------|-------------|
| `cargo build` OOM | Close other apps; in `.wslconfig` increase `memory` for the VMM. |
| Files on `/mnt/c` slow | Clone the repo under the Linux home filesystem (`~/projects`) for faster I/O. |
| Browser / GPU | Chrome for **CDP browser tool** runs on Windows; point **`[browser].url`** at the Windows host IP and open debugging port if needed (advanced). |
| Use Windows `harness.exe` from WSL | Possible but confusing; pick **one** binary (Linux inside WSL **or** Windows native), not both on the same repo. |

---

## After installing

1. **Check health**

   ```bash
   harness doctor
   ```

2. **Set an API key** (pick one)

   ```bash
   export ANTHROPIC_API_KEY="sk-ant-..."    # Unix
   # PowerShell: $env:ANTHROPIC_API_KEY = "sk-ant-..."
   ```

3. **Optional — global config**

   ```bash
   harness init          # creates ~/.harness/config.toml if you use the CLI generator
   ```

   Install scripts may already have created the same template.

4. **Optional — Ollama for defaults**

   ```bash
   ollama pull qwen3-coder:30b
   ollama pull nomic-embed-text
   ```

5. **Run**

   ```bash
   cd /path/to/your/project
   harness
   ```

More day-to-day usage: [`Start Here/USER MANUAL.md`](../Start%20Here/USER%20MANUAL.md).

---

## Optional features (by OS)

Short reference; full table in [README](../README.md).

| Feature | macOS | Linux | Windows native |
|--------|:-----:|:-----:|:--------------:|
| Core CLI / TUI | Yes | Yes | Yes |
| `shell` tool best experience | `sh -c` | `sh -c` | Git `sh` on PATH, or use WSL2 |
| Desktop notifications | Yes | Needs libnotify stack | Limited |
| Voice / Whisper | sox / afrecord | sox | Not first-class |
| Computer use (dangerous) | cliclick | xdotool | Not supported |
| VS Code extension + daemon socket | Yes | Yes | Prefer WSL2 |

---

## Updating

**From a git clone:**

```bash
cd harness
git pull
cargo build --profile release-lto
# copy binary over your old harness the same way you installed it
```

**Re-run the install script** — it rebuilds from the latest `main` when cloning fresh (safe if you are not relying on local patches in the clone).

---

## Uninstall

1. Remove the binary (`~/.local/bin/harness` or `harness.exe` on Windows).  
2. Optionally remove config and data:

   ```bash
   rm -rf ~/.harness
   ```

   On Windows: delete `%USERPROFILE%\.harness`.  

**Warning:** That directory holds sessions, memory, cost DB, and trust rules. Back up first if you care about history.

---

## Security: `curl | bash` and remote scripts

Remote install one-liners are **convenient but trust-sensitive**. Recommended practice:

1. Open the script URL in a browser and read it.  
2. Or clone the repo and run `scripts/install.sh` / `scripts/install.ps1` locally.  
3. Prefer **HTTPS** raw GitHub URLs from the official repo you expect.

---

## Still stuck?

- Run **`harness doctor`** and note any red items.  
- Confirm **Rust**: `rustc --version`, `cargo --version`.  
- Open an issue with **OS version**, **install method**, and the **full error text** (build log or runtime).

CI configuration for reference: [`.github/workflows/ci.yml`](../.github/workflows/ci.yml).
