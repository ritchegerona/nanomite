#!/usr/bin/env bash
#
# file-recover.sh
# Attempts safe, read-only recovery of data from a potentially corrupted NTFS
# volume attached to macOS. Writes recovered files to a destination folder.
#
# Usage:
#   sudo bash file-recover.sh <device_node> <mount_point> <destination>
#
# Example:
#   sudo bash file-recover.sh /dev/disk4s1 /mnt/win /Volumes/USB-X/BACKUP-WORKSPACE

set -euo pipefail

DEV="${1:-}"
MP="${2:-}"
DST="${3:-}"

if [[ -z "$DEV" || -z "$MP" || -z "$DST" ]]; then
    echo "Usage: $0 <device_node> <mount_point> <destination>"
    echo "Example: $0 /dev/disk4s1 /mnt/win /Volumes/USB-X/BACKUP-WORKSPACE"
    exit 1
fi

mkdir -p "$MP" "$DST"
LOG="$DST/recover-$(date +%Y%m%d-%H%M%S).log"

echo "=== USB-X Rescue Kit: File Recovery ===" | tee -a "$LOG"
echo "Device: $DEV" | tee -a "$LOG"
echo "Mount: $MP" | tee -a "$LOG"
echo "Destination: $DST" | tee -a "$LOG"
echo "Date: $(date)" | tee -a "$LOG"
echo "" | tee -a "$LOG"

# Step 1: Unmount if already mounted
if mount | grep -q "$MP"; then
    echo "[*] Unmounting $MP..." | tee -a "$LOG"
    diskutil unmount "$MP" 2>&1 | tee -a "$LOG" || true
fi

# Step 2: Try read-only mount
echo "[*] Attempting read-only mount of $DEV at $MP..." | tee -a "$LOG"
if ! mount -t ntfs -o ro "$DEV" "$MP" 2>>"$LOG"; then
    echo "[!] Standard ntfs mount failed. Trying ntfs-3g read-only..." | tee -a "$LOG"
    if ! ntfs-3g -o remove_hiberfile,ro "$DEV" "$MP" 2>>"$LOG"; then
        echo "[!] Mount failed. Drive may be severely corrupted. Try TestDisk/PhotoRec from SystemRescue." | tee -a "$LOG"
        exit 1
    fi
fi

echo "[+] Mounted read-only at $MP" | tee -a "$LOG"

# Step 3: Copy recoverable user data
USERS_DIR="$MP/Users"
if [[ -d "$USERS_DIR" ]]; then
    for user_dir in "$USERS_DIR"/*/; do
        user_name="$(basename "$user_dir")"
        case "$user_name" in
            Public|Default|"Default User"|"All Users"|Administrator) continue ;;
        esac

        user_dst="$DST/$user_name"
        mkdir -p "$user_dst"

        echo "[*] Recovering $user_name..." | tee -a "$LOG"
        for folder in Documents Desktop Pictures Downloads Music Videos; do
            if [[ -d "$user_dir/$folder" ]]; then
                cp -rv "$user_dir/$folder" "$user_dst/" 2>&1 | tee -a "$LOG"
            fi
        done
    done
else
    echo "[!] No /Users directory found. Listing root contents:" | tee -a "$LOG"
    ls -la "$MP" | tee -a "$LOG"
fi

# Step 4: Unmount
echo "[*] Unmounting $MP..." | tee -a "$LOG"
diskutil unmount "$MP" 2>&1 | tee -a "$LOG" || true

echo "" | tee -a "$LOG"
echo "=== Recovery complete: $LOG ===" | tee -a "$LOG"
