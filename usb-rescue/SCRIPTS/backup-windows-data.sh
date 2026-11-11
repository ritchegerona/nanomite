#!/usr/bin/env bash
#
# backup-windows-data.sh
# Copies Windows user data from a mounted Windows volume to a backup destination.
# Intended to run on macOS (or Linux) with an externally attached Windows drive.
#
# Usage:
#   sudo bash backup-windows-data.sh <source_win_volume> <dest_backup_folder>
#
# Example:
#   sudo bash backup-windows-data.sh /Volumes/Windows /Volumes/USB-X/BACKUP-WORKSPACE

set -euo pipefail

SRC="${1:-}"
DST="${2:-}"

if [[ -z "$SRC" || -z "$DST" ]]; then
    echo "Usage: $0 <source_win_volume> <dest_backup_folder>"
    echo "Example: $0 /Volumes/Windows /Volumes/USB-X/BACKUP-WORKSPACE"
    exit 1
fi

if [[ ! -d "$SRC" ]]; then
    echo "ERROR: Source '$SRC' not found."
    exit 1
fi

mkdir -p "$DST"
LOG="$DST/backup-$(date +%Y%m%d-%H%M%S).log"

echo "=== USB-X Rescue Kit: Windows Data Backup ===" | tee -a "$LOG"
echo "Source: $SRC" | tee -a "$LOG"
echo "Destination: $DST" | tee -a "$LOG"
echo "Date: $(date)" | tee -a "$LOG"
echo "" | tee -a "$LOG"

# Common Windows user folders to recover
USERS_DIR="$SRC/Users"
FOLDERS=(
    "Documents"
    "Desktop"
    "Pictures"
    "Downloads"
    "Music"
    "Videos"
)

# Browser profile locations (relative to each user profile)
BROWSER_PATHS=(
    "AppData/Local/Google/Chrome/User Data"
    "AppData/Roaming/Mozilla/Firefox/Profiles"
    "AppData/Local/Microsoft/Edge/User Data"
)

backup_item() {
    local src_path="$1"
    local dst_path="$2"
    if [[ -e "$src_path" ]]; then
        echo "[+] Copying: $src_path" | tee -a "$LOG"
        cp -rv "$src_path" "$dst_path" 2>&1 | tee -a "$LOG"
    else
        echo "[-] Skipping (not found): $src_path" | tee -a "$LOG"
    fi
}

if [[ -d "$USERS_DIR" ]]; then
    for user_dir in "$USERS_DIR"/*/; do
        user_name="$(basename "$user_dir")"
        # Skip system/default accounts
        case "$user_name" in
            Public|Default|Default User|All Users|Administrator|Public) continue ;;
        esac

        user_dst="$DST/$user_name"
        mkdir -p "$user_dst"

        for folder in "${FOLDERS[@]}"; do
            backup_item "$user_dir/$folder" "$user_dst/"
        done

        for browser in "${BROWSER_PATHS[@]}"; do
            backup_item "$user_dir/$browser" "$user_dst/BROWSER-PROFILES/"
        done
    done
else
    echo "[!] No /Users directory found on source. Is this a Windows drive?" | tee -a "$LOG"
fi

echo "" | tee -a "$LOG"
echo "=== Backup complete: $LOG ===" | tee -a "$LOG"
