# Windows Git Bash shell override

Codex prefers Git Bash on Windows and deliberately avoids the implicit WSL
`bash.exe` shim unless you explicitly point Codex at it.

## What changed

- Codex prefers Git Bash when it is available on native Windows.
- `CODEX_SHELL_PATH` can point Codex at any supported shell binary explicitly.
- `CODEX_GIT_BASH_PATH` can force a specific Git Bash install path.
- `CODEX_SHELL_PATH` takes precedence over `CODEX_GIT_BASH_PATH`.

## Override variables

Use one of the following environment variables:

- `CODEX_SHELL_PATH` - absolute path to the shell binary to use
- `CODEX_GIT_BASH_PATH` - absolute path to a Git Bash `bash.exe`

Example:

```powershell
setx CODEX_GIT_BASH_PATH "C:\Program Files\Git\bin\bash.exe"
```

## Validation

You should see MSYS-style paths from Git Bash instead of WSL-style `/mnt/c/...`
paths:

```shell
codex exec "pwd"
# /c/projects/your-workspace
```

If you still see `/mnt/c/...`, either the WSL launcher is being selected outside
the intended implicit-resolution path or an explicit override is pointing Codex
at it.
