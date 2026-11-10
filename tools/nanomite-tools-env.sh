#!/usr/bin/env bash
# nanomite-tools-env.sh
#
# Source this file to make all nanomite tools available from ANY directory
# on the current machine. Works on Linux and macOS.
#
# Usage:
#   source /path/to/nanomite/tools/nanomite-tools-env.sh
#
# Or add to your shell startup (~/.bashrc, ~/.zshrc, ~/.profile):
#   source /path/to/nanomite/tools/nanomite-tools-env.sh
#
# After sourcing, run from anywhere:
#   whois_rdap example.com --json
#   spiderfoot -t example.com -m sfp_whois
#
# Authorized use only: run against assets you own or have explicit written
# permission to assess. Do not use to identify or dox individuals.

# Resolve the tools directory (this file lives in nanomite/tools/)
ENV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export NANOMITE_TOOLS_HOME="${ENV_DIR}"

# Add the shim/bin dir to PATH. Run install.sh first to create shims,
# or this will add the tools dir itself so you can call python3 directly.
if [[ -d "${ENV_DIR}/custom" ]]; then
    # If shims are installed in ~/.local/bin or a custom bin, prefer that.
    # Otherwise, prepend the tools dir so `python3 tools/custom/...` works
    # via NANOMITE_TOOLS_HOME-aware wrappers.
    :
fi

# Common locations where shims may live
for shim_dir in "${HOME}/.local/bin" "/usr/local/bin" "${ENV_DIR}/bin"; do
    if [[ -d "${shim_dir}" ]]; then
        export PATH="${shim_dir}:${PATH}"
        break
    fi
done

# Convenience alias: run any tool via python3 + NANOMITE_TOOLS_HOME
# Example: nanomite whois_rdap example.com --json
nanomite() {
    if [[ $# -eq 0 ]]; then
        echo "Usage: nanomite <tool-name> [args...]"
        echo "  e.g. nanomite whois_rdap example.com --json"
        echo "  e.g. nanomite spiderfoot -t example.com -m sfp_whois"
        return 1
    fi
    local tool="$1"
    shift
    # Search common shim locations first, then fall back to direct script path
    local found=""
    for shim_dir in "${HOME}/.local/bin" "/usr/local/bin" "${ENV_DIR}/bin"; do
        if [[ -x "${shim_dir}/${tool}" ]]; then
            found="${shim_dir}/${tool}"
            break
        fi
    done
    if [[ -z "${found}" ]]; then
        # Try to find the tool under custom/
        found="$(find "${ENV_DIR}/custom" -type f -name "${tool}.py" -print -quit 2>/dev/null || true)"
        if [[ -n "${found}" ]]; then
            exec python3 "${found}" "$@"
            return $?
        fi
        echo "[!] Tool '${tool}' not found. Run install.sh first." >&2
        return 127
    fi
    exec "${found}" "$@"
}

echo "[+] nanomite-tools env loaded (NANOMITE_TOOLS_HOME=${ENV_DIR})"
echo "[+] Run 'nanomite <tool> [args]' or install shims via install.sh"