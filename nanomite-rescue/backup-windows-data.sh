#!/bin/bash
# USB-X Rescue Kit — Backup Windows User Data
# Usage: sudo bash backup-windows-data.sh
# Run on a Mac with the corrupted Windows drive connected externally

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

USB_MOUNT="/Volumes/USB-X"
BACKUP_DEST="$USB_MOUNT/BACKUP-WORKSPACE"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RECOVERY_DIR="$BACKUP_DEST/RECOVERED-$TIMESTAMP"

echo -e "${GREEN}=== USB-X Rescue Kit: Windows Data Backup ===${NC}"
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then
  echo -e "${RED}Please run with sudo: sudo bash $0${NC}"
  exit 1
fi

# Check if USB-X is mounted
if [ ! -d "$USB_MOUNT" ]; then
  echo -e "${RED}USB-X not found at $USB_MOUNT${NC}"
  echo "Please connect USB-X and try again."
  exit 1
fi

# Create backup destination
mkdir -p "$RECOVERY_DIR"
echo -e "${GREEN}Backup destination: $RECOVERY_DIR${NC}"
echo ""

# List external disks (NTFS/Windows volumes)
echo -e "${YELLOW}Available external disks:${NC}"
diskutil list external | grep -E "Windows_NTFS|Microsoft Basic Data|NTFS|External" || true
echo ""

# Ask user for the Windows disk identifier
read -p "Enter Windows disk identifier (e.g., disk4s2): " WIN_DISK

if [ ! -b "/dev/$WIN_DISK" ]; then
  echo -e "${RED}Device /dev/$WIN_DISK not found${NC}"
  exit 1
fi

# Try to mount the Windows partition
WIN_MOUNT="/mnt/windows-rescue"
mkdir -p "$WIN_MOUNT"

echo -e "${YELLOW}Attempting to mount /dev/$WIN_DISK...${NC}"

# Try ntfs-3g first (read-write), fallback to read-only
if mount -t ntfs-3g -o rw,remove_hiberfile "/dev/$WIN_DISK" "$WIN_MOUNT" 2>/dev/null; then
  echo -e "${GREEN}Mounted read-write${NC}"
elif mount -t ntfs -o ro "/dev/$WIN_DISK" "$WIN_MOUNT" 2>/dev/null; then
  echo -e "${YELLOW}Mounted read-only (hibernated or read-only)${NC}"
else
  echo -e "${RED}Failed to mount /dev/$WIN_DISK${NC}"
  echo "Try: diskutil unmountDisk /dev/$WIN_DISK  then re-run"
  exit 1
fi

# Define Windows user profile folders to backup
WIN_USERS="$WIN_MOUNT/Users"

if [ ! -d "$WIN_USERS" ]; then
  echo -e "${RED}No /Users folder found at $WIN_MOUNT${NC}"
  echo "This may not be a Windows system drive, or path is different."
  umount "$WIN_MOUNT"
  exit 1
fi

echo -e "${GREEN}Found Windows Users directory. Scanning for profiles...${NC}"
echo ""

# Find user directories (exclude Public, Default, All Users, etc.)
USER_DIRS=$(ls -1 "$WIN_USERS" | grep -v -E "^(Public|Default|All Users|Administrator|Public|Default User|template)$" || true)

if [ -z "$USER_DIRS" ]; then
  echo -e "${YELLOW}No standard user profiles found${NC}"
fi

# Folders to copy per user
FOLDERS=("Documents" "Desktop" "Pictures" "Downloads" "Favorites" "Music" "Videos")

TOTAL_COPIED=0

for user in $USER_DIRS; do
  USER_PATH="$WIN_USERS/$user"
  if [ ! -d "$USER_PATH" ]; then
    continue
  fi

  echo -e "${YELLOW}Processing user: $user${NC}"
  USER_DEST="$RECOVERY_DIR/Users/$user"
  mkdir -p "$USER_DEST"

  for folder in "${FOLDERS[@]}"; do
    SRC="$USER_PATH/$folder"
    if [ -d "$SRC" ] && [ "$(ls -A "$SRC" 2>/dev/null)" ]; then
      echo -e "  ${GREEN}Copying: $folder${NC}"
      cp -av "$SRC" "$USER_DEST/" 2>/dev/null || true
      FILES_COUNT=$(find "$USER_DEST/$folder" -type f 2>/dev/null | wc -l)
      TOTAL_COPIED=$((TOTAL_COPIED + FILES_COUNT))
      echo -e "    ${GREEN}✓ $FILES_COUNT files${NC}"
    fi
  done

  # Copy Outlook data if exists
  OUTLOOK="$USER_PATH/AppData/Roaming/Microsoft/Outlook"
  if [ -d "$OUTLOOK" ]; then
    echo -e "  ${YELLOW}Copying: Outlook data${NC}"
    cp -av "$OUTLOOK" "$USER_DEST/AppData-Roaming-Microsoft-Outlook" 2>/dev/null || true
  fi

  # Copy Firefox/Chrome profiles (AppData/Local and Roaming)
  for appdata in "AppData/Local" "AppData/Roaming"; do
    APPDATA_PATH="$USER_PATH/$appdata"
    if [ -d "$APPDATA_PATH" ]; then
      # Chrome
      if [ -d "$APPDATA_PATH/Google/Chrome/User Data" ]; then
        echo -e "  ${YELLOW}Copying: Chrome profile${NC}"
        cp -av "$APPDATA_PATH/Google/Chrome" "$USER_DEST/$appdata-Chrome" 2>/dev/null || true
      fi
      # Firefox
      if [ -d "$APPDATA_PATH/Mozilla/Firefox/Profiles" ]; then
        echo -e "  ${YELLOW}Copying: Firefox profile${NC}"
        cp -av "$APPDATA_PATH/Mozilla/Firefox" "$USER_DEST/$appdata-Firefox" 2>/dev/null || true
      fi
    fi
  done

  echo ""
done

# Copy entire Windows drive as fallback (optional, large)
echo -e "${YELLOW}Quick scan complete. Total files recovered: $TOTAL_COPIED${NC}"
echo ""

# Unmount Windows drive
umount "$WIN_MOUNT" 2>/dev/null || true
rmdir "$WIN_MOUNT" 2>/dev/null || true

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}Backup complete!${NC}"
echo -e "${GREEN}Location: $RECOVERY_DIR${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo -e "${YELLOW}Next steps:${NC}"
echo "1. Verify recovered files in $RECOVERY_DIR"
echo "2. If drive is severely corrupted, try PhotoRec (in RESCUE-ISO/)"
echo "3. After confirming backup, you can safely reinstall/repair Windows"