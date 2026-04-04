# Windows Git Bash shell override

Codex prefers Git Bash on Windows and deliberately avoids the WSL `bash.exe`
shim unless you explicitly point Codex at it.

## What changed

- Codex prefers Git Bash when available on Windows.
- `CODEX_SHELL_PATH` can point Codex at any shell binary explicitly.
- `CODEX_GIT_BASH_PATH` can force a specific Git Bash install path.

## Configuration

Use one of the following environment variables:

- `CODEX_SHELL_PATH` - absolute path to the shell binary to use
- `CODEX_GIT_BASH_PATH` - absolute path to a Git Bash `bash.exe`

Example:

```powershell
setx CODEX_GIT_BASH_PATH "C:\Program Files\Git\bin\bash.exe"
```

## Validation

You should see MSYS paths (Git Bash) instead of `/mnt/c/...` (WSL):

```shell
codex exec "pwd"
# /c/projects/your-workspace
```

If you still see `/mnt/c/...`, the WSL `bash.exe` is still being resolved first
or an explicit override is pointing Codex at it.
