# USB-X Rescue Kit
Bootable Windows recovery + backup toolkit on USB-X.

## Layout (on USB-X)
- `RESCUE-ISO/` — Drop SystemRescue ISO here (~600 MB) to make USB-X bootable
- `SCRIPTS/` — macOS/Linux helper scripts for data backup & recovery
- `TOOLS-WINDOWS/` — Add portable .exe tools (Rufus, TestDisk, Recuva, etc.)
- `BACKUP-WORKSPACE/` — Destination folder for recovered files
- `DOCS/RESCUE-PLAYBOOK.md` — Step-by-step recovery guide

## First-time setup
1. Download SystemRescue ISO: https://system-rescue-linux.org/
2. Save it to `RESCUE-ISO/systemrescue.iso`
3. Make USB-X bootable:
   - **Windows:** Rufus → select USB-X → ISO image → systemrescue.iso → START
   - **Mac/Linux:** `sudo dd if=RESCUE-ISO/systemrescue.iso of=/dev/diskX bs=4m` (replace diskX with your USB device)
4. Copy `SCRIPTS/*` and `DOCS/*` to USB-X if not already there

## Use on a Mac (corrupted Windows drive connected externally)
```bash
sudo bash /Volumes/USB-X/SCRIPTS/backup-windows-data.sh /Volumes/WindowsDrive /Volumes/USB-X/BACKUP-WORKSPACE
```
