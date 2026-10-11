# Try Harness 1.4.0-beta.2

Harness is a free MIT work assistant with a terminal and local browser UI. This
candidate provides native command-line downloads for developer evaluation and
personal use. Model work needs a local backend or your own provider account.
You can prepare a project brief without either.

## Download the candidate

Open the [v1.4.0-beta.2 release](https://github.com/seanebones-lang/harness/releases/tag/v1.4.0-beta.2).
Choose the binary for your machine:

| Machine | Download |
| --- | --- |
| Mac with Apple Silicon | `harness-macos-aarch64` |
| Intel Mac | `harness-macos-x86_64` |
| Linux with glibc 2.39+, x86-64 | `harness-linux-x86_64` |
| Linux with glibc 2.39+, ARM64 | `harness-linux-aarch64` |
| Windows x86-64 | `harness-windows-x86_64.exe` |

The Linux downloads require glibc 2.39 or later, confirmed from their ELF
version-needs records. Native checks ran on Ubuntu 24.04. For an older glibc
distribution, use the pinned source path and validate the build on that machine.

The Windows x86-64 executable imports `VCRUNTIME140.dll`. Its matching Microsoft
Visual C++ v14 x64 Redistributable must be present. If the runtime is missing,
use [Microsoft's official runtime download page](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist).
Running the native binary does not require Visual Studio or Rust. CI's Windows
host already has developer tooling; a successful native check does not prove
this prerequisite is present on a fresh consumer machine.

Also download `checksums.txt`, `LICENSE.txt` and `THIRD-PARTY-LICENSES.html`.
Keep the license files with redistributed binaries. `RELEASE-MANIFEST.json`
records the source commit, target names and asset hashes. This is a **prerelease**;
GitHub's unqualified latest-release link may still select historical v1.3.0.

Verify the binary's SHA-256 against its exact line in `checksums.txt` **before
running it**. On macOS use `shasum -a 256 DOWNLOAD`; on Linux use `sha256sum DOWNLOAD`;
in PowerShell use `Get-FileHash DOWNLOAD -Algorithm SHA256`.

On a Mac or Linux, put the verified binary on your PATH:

```sh
mkdir -p "$HOME/.local/bin"
install -m 755 ./harness-macos-aarch64 "$HOME/.local/bin/harness"
export PATH="$HOME/.local/bin:$PATH"
harness --version
```

Replace the example filename with your machine's download. On Windows, copy the
verified download to a user PATH directory as `harness.exe`, then open a new
terminal. You can instead run it by its full path. These command-line downloads
are not signed/notarized desktop installers; do not bypass operating-system
security warnings. A source build is available below.

The release's `install.sh` / `install.ps1` can install a specific candidate after
you download, checksum and review the script:

```sh
bash ./install.sh v1.4.0-beta.2
```

```powershell
.\install.ps1 v1.4.0-beta.2
```

The scripts verify binary checksums and install to your user directory. Download
failure can trigger a source build, which needs Git and the pinned Rust toolchain.
They do not configure a provider or copy a credential. See [INSTALL.md](INSTALL.md)
for PATH, platform tooling, updates and removal.

## First success without a model

Choose an existing work folder, or create a disposable one:

```sh
mkdir -p "$HOME/harness-first-try"
harness -C "$HOME/harness-first-try" work --kind software \
  "Add a small utility with a runnable test" --prepare-only
```

Inspect `.harness/build.toml` inside that folder. It retains the outcome and
acceptance checks. The matching workflow is installed in `.agents/skills/`.
This step makes no model request and does not implement the task.

You can use `--kind research`, `documents`, `website`, `automation` or `apple`
instead. Research/document preparation does not infer repository test commands.
Supply checks that make sense for your project.

## Connect your model and continue

```sh
harness -C "$HOME/harness-first-try" setup
harness -C "$HOME/harness-first-try" route show
harness -C "$HOME/harness-first-try" doctor
harness -C "$HOME/harness-first-try" work
```

Select your provider and exact model in setup. Make its named credential
available outside the repository, or start your chosen local backend. Setup
stores credential environment-variable names; it does not create a provider
account or provision a model. Provider usage may cost money.

`doctor` reports missing dependencies; optional integrations need their own setup.
Read the run's output and inspect the actual files/checks. A finished model turn
is not proof that acceptance criteria passed. `harness sessions` and
`harness --resume SESSION_ID work` let you inspect and continue saved work.

## Terminal and browser

From your selected work folder, run `harness` for the terminal interface. Use
**Ctrl+Q** to quit. To start the browser interface:

```sh
harness -C "$HOME/harness-first-try" serve --addr 127.0.0.1:8787
```

Open the printed loopback URL. Authenticate with the local server token using
the UI if prompted. Keep the token private. Stop the server with Ctrl+C.
The provider route is shared with CLI work; the browser needs no hosted Harness
account. See the [user manual](../Start%20Here/USER%20MANUAL.md).

## Build the pinned source

```sh
git clone --branch v1.4.0-beta.2 --depth 1 https://github.com/seanebones-lang/harness.git
cd harness
cargo build --locked --profile release-lto
./target/release-lto/harness --version
```

Git and [rustup](https://rustup.rs) are needed; `rust-toolchain.toml` pins Rust
1.95.0. Native compilation needs your operating system's compiler/linker tooling.
Alternatively extract the versioned source archive and build from its root.
Use `make verify` for development checks; it needs Python 3 and test tooling
in addition to Rust. Model credentials are unnecessary for its synthetic checks.

## Scope of this candidate

The release matrix tests the final CLI binaries on five native targets with
workspace and synthetic CLI/HTTP/build/work checks. Synthetic fixtures establish
transport, tool execution, persistence and recovery behavior, not model quality
or autonomous project success. Interactive and fresh-machine acceptance on every
OS, signed desktop packages, and every optional integration remain separate.

The default build includes the pinned DeadBolt admission adapter. It checks
operator policy after review and before registered tool effects. Harness uses
legacy admission; exact-only tools are refused until an exact-action adapter
exists. [DEADBOLT.md](DEADBOLT.md) explains its boundary. Shell authority and
post-write hooks are not arbitrary-code isolation. Keep work in version control
and review changes you care about.
