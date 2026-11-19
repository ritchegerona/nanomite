#!/bin/bash
# USB-X Rescue Kit — File Recovery Helper
# Usage: sudo bash file-recover.sh <device> <destination>
# Example: sudo bash file-recover.sh /dev/disk4s2 /Volumes/USB-X/BACKUP-WORKSPACE

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

if [ "$EUID" -ne 0 ]; then
  echo -e "${RED}Please run with sudo${NC}"
  exit 1
fi

SRC_DEV="$1"
DEST_DIR="$2"

if [ -z "$SRC_DEV" ] || [ -z "$DEST_DIR" ]; then
  echo "Usage: sudo bash $0 <device> <destination>"
  echo "Example: sudo bash $0 /dev/disk4s2 /Volumes/USB-X/BACKUP-WORKSPACE"
  exit 1
fi

if [ ! -b "$SRC_DEV" ]; then
  echo -e "${RED}Device $SRC_DEV not found${NC}"
  exit 1
fi

mkdir -p "$DEST_DIR"

echo -e "${GREEN}=== USB-X File Recovery ===${NC}"
echo "Source: $SRC_DEV"
echo "Destination: $DEST_DIR"
echo ""

# Try to mount read-only first
MOUNT_DIR="/mnt/recover-src"
mkdir -p "$MOUNT_DIR"

echo -e "${YELLOW}Mounting $SRC_DEV read-only...${NC}"
if mount -t ntfs -o ro "$SRC_DEV" "$MOUNT_DIR" 2>/dev/null; then
  echo -e "${GREEN}Mounted read-only${NC}"
  
  # List top-level directories
  echo -e "${YELLOW}Top-level folders on source:${NC}"
  ls -1 "$MOUNT_DIR" | head -30
  echo ""

  read -p "Copy entire drive? (y/n): " COPY_ALL
  if [ "$COPY_ALL" = "y" ]; then
    echo -e "${YELLOW}Copying all data... (this may take a while)${NC}"
    cp -av "$MOUNT_DIR" "$DEST_DIR/drive-copy-$(date +%Y%m%d)/" 2>/dev/null || true
    echo -e "${GREEN}Done${NC}"
  fi

  umount "$MOUNT_DIR" 2>/dev/null || true
else
  echo -e "${RED}Cannot mount $SRC_DEV${NC}"
  echo "The filesystem may be severely corrupted."
  echo ""
  echo -e "${YELLOW}Recommendation:${NC}"
  echo "1. Boot from RESCUE-ISO/ (SystemRescue) on this USB-X"
  echo "2. Use PhotoRec/testdisk to recover files at the filesystem level"
  echo "3. Or use ddrescue to create an image: ddrescue -f -n $SRC_DEV $DEST_DIR/image.img $DEST_DIR/map.log"
fi

rmdir "$MOUNT_DIR" 2>/dev/null || true
echo ""
echo -e "${GREEN}Recovery helper finished${NC}"