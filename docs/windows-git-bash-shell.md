# Windows Git Bash shell override

This repo now supports forcing Codex to use Git Bash on Windows instead of the WSL shim.

## What changed

- Codex prefers Git Bash when available on Windows.
- You can override the shell path via config or environment variables.

## Configuration

Use one of the following options (highest priority first):

1) Config overrides

Add to `~/.codex/config.toml`:

```
shell_path = "C:\\Program Files\\Git\\bin\\bash.exe"
```

2) Environment variables

- `CODEX_SHELL_PATH` (absolute path to the shell binary)
- `CODEX_GIT_BASH_PATH` (absolute path to Git Bash)

Example:

```
setx CODEX_GIT_BASH_PATH "C:\\Program Files\\Git\\bin\\bash.exe"
```

## Validation

You should see MSYS paths (Git Bash) instead of `/mnt/c/...` (WSL):

```
codex exec "pwd"
# /c/projects/your-workspace
```

If you still see `/mnt/c/...`, the WSL `bash.exe` is still being resolved first on PATH and the override is not set.
