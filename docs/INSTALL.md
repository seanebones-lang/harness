# Install NextEleven Harness

For the current **MIT-licensed 1.4.0-beta.2 developer prerelease**, start with the
[version-pinned quickstart](QUICKSTART.md). It covers the five native downloads,
checksum verification and a first success without a model account. This guide
adds platform requirements, PATH, troubleshooting and removal.

This is a developer evaluation release. Native CI and synthetic checks are
recorded in the [release](https://github.com/seanebones-lang/harness/releases/tag/v1.4.0-beta.2);
they do not establish interactive or fresh-machine acceptance on every OS.
The CLI downloads are separate from optional desktop/editor sources.

## Choose your installation path

| Path | Required |
| --- | --- |
| Verified native CLI binary | Matching OS/architecture and runtime libraries, a terminal and SHA-256 verification; no Rust compiler |
| Version-pinned installer | Reviewed and checksum-verified `install.sh` or `install.ps1`; it verifies the matching binary and can fall back to source compilation if download fails |
| Source build | Git, rustup with pinned Rust 1.95.0, and the platform's native compiler/linker tooling |
| Model work | Your selected local backend or provider account and exact provider:model route; usage may cost money |
| Offline brief preparation | No backend, provider account or API key |

Git is recommended for repository work and recovery. On Windows, Git for Windows
also supplies the POSIX shell used by the shell-tool acceptance fixtures. Optional
integrations need their own programs and permissions; `harness doctor` reports
what it can detect.

## macOS

Choose `harness-macos-aarch64` for Apple Silicon or `harness-macos-x86_64` for Intel.
Follow the quickstart to verify the download before executing it. Install to a
user-owned directory:

```sh
mkdir -p "$HOME/.local/bin"
install -m 755 ./harness-macos-aarch64 "$HOME/.local/bin/harness"
export PATH="$HOME/.local/bin:$PATH"
harness --version
```

Substitute the Intel filename when appropriate. To retain PATH across terminals,
add the export line once to your shell profile, such as `~/.zshrc`, and open a new
terminal. Source compilation additionally needs Xcode command-line tools and
rustup. Downloaded CLI binaries are not signed/notarized desktop installers.
Do not bypass operating-system security warnings; the pinned source path is an
alternative.

## Linux

Choose the glibc binary matching x86-64 or ARM64. Both downloads require
**glibc 2.39 or later**, confirmed in their ELF version-needs records. For an older
glibc distribution, use the pinned source path and validate the resulting binary
on that machine. Verify the download checksum, then use the
same user-directory installation steps as macOS with your Linux filename. Add
`~/.local/bin` to your shell profile if needed. Alpine/musl is outside this native
binary matrix; do not assume a glibc download is compatible.

The native Linux builders use Ubuntu 24.04. A different distribution needs
compatible runtime libraries; the glibc label does not establish compatibility
with every glibc release.

Source builds need the distribution's native C/C++ compiler, linker and Git,
plus rustup. Package names differ by distribution. Begin in a writable folder;
the pinned toolchain and locked dependency graph are used by the command below.

## Windows

Choose `harness-windows-x86_64.exe`. This release imports `VCRUNTIME140.dll` and
requires a compatible Microsoft Visual C++ v14 x64 Redistributable. Install the
matching runtime from [Microsoft's official download page](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist)
if it is missing; use a runtime version at least as recent as the build tools
used for the application. This is separate from installing Visual Studio.
The native CI host already has developer tooling, so it does not establish that
a fresh Windows consumer machine has this prerequisite.

In PowerShell, verify the executable against the matching
line of `checksums.txt`:

```powershell
Get-FileHash .\harness-windows-x86_64.exe -Algorithm SHA256
```

After verifying, copy it to a user-owned directory as `harness.exe`, or run the
verified file by its full path. To use `%USERPROFILE%\.local\bin`, add that folder
to **User Path** in Environment Variables and open a new terminal. A native
binary does not require Visual Studio or Rust. Source builds require the MSVC
Rust host toolchain and matching C++ build tools/Windows SDK.

Git for Windows is recommended for the shell tool. With no POSIX shell, Harness
falls back to PowerShell and then Command Prompt; POSIX verification commands
may need adjustment. The five-target release fixtures do not prove every
PowerShell/Command Prompt fallback behavior.

The release's PowerShell installer can be reviewed and checksum-verified before
running it with the explicit version, as shown in the quickstart. If local script
policy blocks it, use the verified binary directly rather than weakening a
machine-wide protection. Do not disable antivirus to install or build Harness.

## WSL2

Inside an existing WSL2 Linux distribution, follow the Linux path and keep your
work in its Linux filesystem. Use the Linux binary inside WSL2 or the native
Windows binary from Windows; choose one workflow for a given project. Native
Linux CI is not separate WSL2 interactive acceptance.

## Build the pinned source

```sh
git clone --branch v1.4.0-beta.2 --depth 1 https://github.com/seanebones-lang/harness.git
cd harness
cargo build --locked --profile release-lto
./target/release-lto/harness --version
```

On Windows the executable is `target\release-lto\harness.exe`.
`rust-toolchain.toml` selects Rust 1.95.0 through rustup. Building a different
checkout may produce a different candidate; the versioned source archive and
`RELEASE-MANIFEST.json` identify the published commit. Retain `LICENSE.txt` and
`THIRD-PARTY-LICENSES.html` when redistributing downloaded binaries.

## First use and model setup

Follow [First success without a model](QUICKSTART.md#first-success-without-a-model)
to save a brief and install its workflow with `work --prepare-only`. That prepares
instructions; it does not implement the task.

For model work, run:

```sh
harness -C /path/to/work setup
harness -C /path/to/work route show
harness -C /path/to/work doctor
harness -C /path/to/work work
```

Setup selects your exact route and named credential environment variable. It
does not create a provider account, provision a local model or copy a key.
Keep credentials outside the repository. Config lives at `~/.harness/config.toml`,
with project overrides in `.harness/config.toml`. Installation does not select a
preferred vendor. Check actual files, tool results and acceptance commands;
a completed turn is not proof of completion.

Run `harness` for the terminal interface and quit with **Ctrl+Q**. For the local
browser interface, use `harness serve --addr 127.0.0.1:8787` from your work folder.
Open the printed URL, keep its local token private and stop the server with
Ctrl+C. The [user manual](../Start%20Here/USER%20MANUAL.md) covers further features.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Command not found | Confirm the installed path and User PATH; reopen the terminal. Run the binary by full path to separate installation from PATH. |
| Missing VCRUNTIME140.dll on Windows | Check the Microsoft Visual C++ v14 x64 runtime prerequisite above and use Microsoft's official download. |
| Wrong version | Use `command -v harness` on Unix or `Get-Command harness` in PowerShell to find an older executable. Verify the chosen executable reports `1.4.0-beta.2`. |
| Historical download | Use the explicit prerelease tag. GitHub's unqualified latest-release URL may still select v1.3.0. |
| Source build fails | Check the pinned Rust toolchain, native linker and complete error. Avoid changing platform protections as a workaround. |
| Route not configured | Run setup in the intended folder, then `route show`. Offline preparation remains available. |
| Tool denied | Treat the action as failed even if model prose claims success. Inspect the denial and operator state in [DeadBolt](DEADBOLT.md); do not disable admission checks to hide it. |
| Storage errors | Check free space and permissions on the selected workspace and `~/.harness`. |
| Optional integration unavailable | Check its own setup and the relevant manual; installing the CLI does not provision every integration. |

## Update or remove

Before updating, record the installed executable path and retain the previous
verified version if you need rollback. Download an explicit release, verify its
checksum and replace only that executable. Keep configuration and sessions.
Homebrew metadata in this source tree is not a published official tap.

To remove the CLI, delete the installed `harness` / `harness.exe` from its known
user directory. Config and saved sessions remain in `~/.harness`; removing that
folder is a separate data-deletion decision. Back up anything you want to keep.
