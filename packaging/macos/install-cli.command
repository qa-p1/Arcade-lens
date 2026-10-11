#!/usr/bin/env bash
# Explicit, optional CLI setup for the app already copied to Applications.
set -euo pipefail

app=''
for candidate in "/Applications/Arcade Lens.app" "$HOME/Applications/Arcade Lens.app"; do
  if [[ -x "$candidate/Contents/MacOS/arcade-lens" ]]; then
    app=$candidate
    break
  fi
done
if [[ -z "$app" ]]; then
  echo 'Move Arcade Lens.app to /Applications or ~/Applications before installing its CLI.' >&2
  exit 1
fi

exe="$app/Contents/MacOS/arcade-lens"
bin="$HOME/.local/bin"
shim="$bin/arcade-lens"
case ":${PATH:-}:" in
  *":$bin:"*)
    if [[ -e "$shim" || -L "$shim" ]]; then
      if [[ -L "$shim" && "$(readlink "$shim")" == "$exe" ]]; then
        echo "Arcade Lens CLI is already installed at $shim"
        exit 0
      fi
      echo "Not replacing the existing path: $shim" >&2
      exit 1
    fi
    mkdir -p "$bin"
    ln -s "$exe" "$shim"
    echo "Installed Arcade Lens CLI at $shim. No shell profile was changed."
    ;;
  *)
    echo "Your PATH does not include $bin; no file was changed."
    echo 'For this terminal, add the directory and create the symlink with:'
    printf 'mkdir -p %q\nln -s %q %q\n' "$bin" "$exe" "$shim"
    case "$(basename "${SHELL:-bash}")" in
      fish) printf 'set -gx PATH %q $PATH\n' "$bin" ;;
      *) printf 'export PATH=%q:"$PATH"\n' "$bin" ;;
    esac
    ;;
esac
